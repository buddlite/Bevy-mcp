use bevy_mcp_server::AgentBevyMcpServer;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ServerInfo,
    Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;

use crate::process_manager::{ProcessError, ProcessManager};

fn format_error(error: ProcessError) -> String {
    error.to_json().to_string()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProcessLogsParams {
    #[schemars(description = "Optional stream filter: stdout or stderr.")]
    pub stream: Option<String>,
    #[schemars(description = "Maximum newest log lines to return (default 200).")]
    pub limit: Option<u32>,
}

#[derive(Clone)]
pub struct ProcessToolServer {
    manager: ProcessManager,
}

impl ProcessToolServer {
    fn new(manager: ProcessManager) -> Self {
        Self { manager }
    }
}

#[tool_router(server_handler)]
impl ProcessToolServer {
    #[tool(
        description = "Return managed/external process ownership plus process, transport, and Bevy-host readiness state."
    )]
    async fn process_status(&self) -> String {
        serde_json::to_string(&self.manager.status().await).unwrap()
    }

    #[tool(
        description = "Launch the game executable preconfigured when the supervisor started. Success is returned only after authenticated Bevy host readiness."
    )]
    async fn process_launch(&self) -> String {
        match self.manager.launch().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_error(error),
        }
    }

    #[tool(
        description = "Gracefully stop the supervisor-owned game, then escalate to whole-process-tree termination if necessary. External games are never killed."
    )]
    async fn process_stop(&self) -> String {
        match self.manager.stop().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_error(error),
        }
    }

    #[tool(
        description = "Restart the supervisor-owned game without rebuilding it. Every restart receives a new instance_id and must pass host readiness again."
    )]
    async fn process_restart(&self) -> String {
        match self.manager.restart().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_error(error),
        }
    }

    #[tool(
        description = "Return bounded captured stdout/stderr from the managed game. Game output never shares MCP stdout."
    )]
    async fn process_logs(&self, Parameters(params): Parameters<ProcessLogsParams>) -> String {
        match self.manager.logs(
            params.stream.as_deref(),
            params.limit.unwrap_or(200) as usize,
        ) {
            Ok(logs) => serde_json::json!({ "logs": logs, "count": logs.len() }).to_string(),
            Err(error) => format_error(error),
        }
    }
}

#[derive(Clone)]
pub struct SupervisorMcpServer {
    base: AgentBevyMcpServer,
    process: ProcessToolServer,
}

impl SupervisorMcpServer {
    pub fn new(base: AgentBevyMcpServer, manager: ProcessManager) -> Self {
        Self {
            base,
            process: ProcessToolServer::new(manager),
        }
    }
}

impl ServerHandler for SupervisorMcpServer {
    fn get_info(&self) -> ServerInfo {
        self.base.get_info()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if self.process.get_tool(request.name.as_ref()).is_some() {
            self.process.call_tool(request, context).await
        } else {
            self.base.call_tool(request, context).await
        }
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut base = self
            .base
            .list_tools(request.clone(), context.clone())
            .await?;
        let process = self.process.list_tools(request, context).await?;
        base.tools.extend(process.tools);
        Ok(base)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.process
            .get_tool(name)
            .or_else(|| self.base.get_tool(name))
    }
}
