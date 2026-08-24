use std::collections::HashSet;

use bevy_mcp_server::AgentBevyMcpServer;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ServerInfo,
    Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;

use crate::cargo_executor::{CargoExecutor, CargoInvocation};
use crate::permissions::SupervisorPermissions;
use crate::process_manager::{ProcessError, ProcessManager};

fn format_process_error(error: ProcessError) -> String {
    error.to_json().to_string()
}

fn permission_error(operation: &str) -> String {
    serde_json::json!({
        "error": "SUPERVISOR_PERMISSION_DENIED",
        "message": format!("Supervisor permission for {operation} is disabled"),
    })
    .to_string()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProcessLogsParams {
    #[schemars(description = "Optional stream filter: stdout or stderr.")]
    pub stream: Option<String>,
    #[schemars(description = "Maximum newest log lines to return (default 200).")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CargoToolParams {
    #[schemars(
        description = "Cargo package name. Required when project metadata has multiple viable binary packages."
    )]
    pub package: Option<String>,
    #[schemars(
        description = "Cargo binary target. Required when the selected package has multiple binaries."
    )]
    pub bin: Option<String>,
    #[schemars(description = "Cargo profile: dev or release (default dev).")]
    pub profile: Option<String>,
    #[schemars(description = "Cargo features validated against metadata before execution.")]
    pub features: Option<Vec<String>>,
}

impl CargoToolParams {
    fn into_invocation(self) -> CargoInvocation {
        CargoInvocation::new(self.package, self.bin, self.profile, self.features, None)
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CargoTestParams {
    #[schemars(
        description = "Cargo package name. Required when project metadata has multiple viable binary packages."
    )]
    pub package: Option<String>,
    #[schemars(
        description = "Cargo binary target. Required when the selected package has multiple binaries."
    )]
    pub bin: Option<String>,
    #[schemars(description = "Cargo profile: dev or release (default dev).")]
    pub profile: Option<String>,
    #[schemars(description = "Cargo features validated against metadata before execution.")]
    pub features: Option<Vec<String>>,
    #[schemars(
        description = "Optional Rust test harness filter passed after Cargo's -- separator."
    )]
    pub filter: Option<String>,
}

impl CargoTestParams {
    fn into_invocation(self) -> CargoInvocation {
        CargoInvocation::new(
            self.package,
            self.bin,
            self.profile,
            self.features,
            self.filter,
        )
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OperationStatusParams {
    #[schemars(description = "Operation ID to check. Omit to list supervisor Cargo operations.")]
    pub operation_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OperationCancelParams {
    #[schemars(description = "Operation ID to cancel.")]
    pub operation_id: String,
}

#[derive(Clone)]
pub struct SupervisorToolServer {
    manager: ProcessManager,
    cargo: CargoExecutor,
    permissions: SupervisorPermissions,
}

impl SupervisorToolServer {
    fn new(
        manager: ProcessManager,
        cargo: CargoExecutor,
        permissions: SupervisorPermissions,
    ) -> Self {
        Self {
            manager,
            cargo,
            permissions,
        }
    }
}

#[tool_router(server_handler)]
impl SupervisorToolServer {
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
        if !self.permissions.process_launch {
            return permission_error("process_launch");
        }
        match self.manager.launch().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_process_error(error),
        }
    }

    #[tool(
        description = "Gracefully stop the supervisor-owned game, then escalate to whole-process-tree termination if necessary. External games are never killed."
    )]
    async fn process_stop(&self) -> String {
        if !self.permissions.process_stop {
            return permission_error("process_stop");
        }
        match self.manager.stop().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_process_error(error),
        }
    }

    #[tool(
        description = "Restart the supervisor-owned game without rebuilding it. Every restart receives a new instance_id and must pass host readiness again."
    )]
    async fn process_restart(&self) -> String {
        if !self.permissions.process_restart {
            return permission_error("process_restart");
        }
        match self.manager.restart().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_process_error(error),
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
            Err(error) => format_process_error(error),
        }
    }

    #[tool(
        description = "Start an allowlisted cargo check operation for the configured project. Returns immediately with a supervisor operation ID."
    )]
    async fn build_check(&self, Parameters(params): Parameters<CargoToolParams>) -> String {
        match self.cargo.start_check(params.into_invocation()) {
            Ok(operation) => serde_json::to_string(&operation).unwrap(),
            Err(error) => error.to_json().to_string(),
        }
    }

    #[tool(
        description = "Start an allowlisted cargo build operation for the configured project. Returns immediately with a supervisor operation ID."
    )]
    async fn build(&self, Parameters(params): Parameters<CargoToolParams>) -> String {
        match self.cargo.start_build(params.into_invocation()) {
            Ok(operation) => serde_json::to_string(&operation).unwrap(),
            Err(error) => error.to_json().to_string(),
        }
    }

    #[tool(
        description = "Start an allowlisted cargo test operation for the configured project. Returns immediately with a supervisor operation ID."
    )]
    async fn test(&self, Parameters(params): Parameters<CargoTestParams>) -> String {
        match self.cargo.start_test(params.into_invocation()) {
            Ok(operation) => serde_json::to_string(&operation).unwrap(),
            Err(error) => error.to_json().to_string(),
        }
    }

    #[tool(
        description = "Read a supervisor Cargo operation by ID, or list all supervisor Cargo operations when no ID is supplied. Game operation IDs continue to route to the Bevy host."
    )]
    async fn operation_status(
        &self,
        Parameters(params): Parameters<OperationStatusParams>,
    ) -> String {
        match self.cargo.status(params.operation_id.as_deref()) {
            Ok(mut operations) if params.operation_id.is_some() => {
                serde_json::to_string(&operations.remove(0)).unwrap()
            }
            Ok(operations) => serde_json::json!({
                "operations": operations,
                "count": operations.len(),
            })
            .to_string(),
            Err(error) => error.to_json().to_string(),
        }
    }

    #[tool(
        description = "Cancel a supervisor Cargo operation and terminate its owned Cargo process tree. Game operation IDs continue to route to the Bevy host."
    )]
    async fn operation_cancel(
        &self,
        Parameters(params): Parameters<OperationCancelParams>,
    ) -> String {
        match self.cargo.cancel(&params.operation_id).await {
            Ok(operation) => serde_json::to_string(&operation).unwrap(),
            Err(error) => error.to_json().to_string(),
        }
    }
}

#[derive(Clone)]
pub struct SupervisorMcpServer {
    base: AgentBevyMcpServer,
    supervisor: SupervisorToolServer,
}

impl SupervisorMcpServer {
    pub fn new(
        base: AgentBevyMcpServer,
        manager: ProcessManager,
        cargo: CargoExecutor,
        permissions: SupervisorPermissions,
    ) -> Self {
        Self {
            base,
            supervisor: SupervisorToolServer::new(manager, cargo, permissions),
        }
    }
}

fn operation_id_from_request(request: &CallToolRequestParams) -> Option<&str> {
    request.arguments.as_ref()?.get("operation_id")?.as_str()
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
        if matches!(
            request.name.as_ref(),
            "operation_status" | "operation_cancel"
        ) {
            if let Some(operation_id) = operation_id_from_request(&request) {
                if !operation_id.starts_with("supervisor:") {
                    return self.base.call_tool(request, context).await;
                }
            }
        }
        if self.supervisor.get_tool(request.name.as_ref()).is_some() {
            self.supervisor.call_tool(request, context).await
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
        let supervisor = self.supervisor.list_tools(request, context).await?;
        let supervisor_names: HashSet<String> = supervisor
            .tools
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        base.tools
            .retain(|tool| !supervisor_names.contains(tool.name.as_ref()));
        base.tools.extend(supervisor.tools);
        Ok(base)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.supervisor
            .get_tool(name)
            .or_else(|| self.base.get_tool(name))
    }
}
