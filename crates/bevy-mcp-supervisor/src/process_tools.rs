use std::collections::HashSet;
use std::time::Duration;

use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_server::AgentBevyMcpServer;
use bevy_mcp_server::backend::GameCommandBackend;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ServerInfo,
    Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::cargo_executor::{CargoError, CargoExecutor, CargoInvocation};
use crate::development_status::DevelopmentStatus;
use crate::permissions::SupervisorPermissions;
use crate::process_manager::{
    ProcessError, ProcessManager, ProcessOwnership, ProcessSnapshot, ProcessState,
};
use crate::rebuild_restart::RebuildRestartCoordinator;

fn format_process_error(error: ProcessError) -> String {
    error.to_json().to_string()
}

fn permission_error(operation: &str) -> String {
    json!({
        "error": "SUPERVISOR_PERMISSION_DENIED",
        "message": format!("Supervisor permission for {operation} is disabled"),
    })
    .to_string()
}

fn capability(implemented: bool, available: bool, allowed: bool) -> Value {
    json!({
        "implemented": implemented,
        "available": available,
        "allowed": allowed,
        "operational": implemented && available && allowed,
    })
}

fn object_field<'a>(root: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    if !root.is_object() {
        *root = json!({});
    }
    let root_object = root.as_object_mut().expect("root was normalized to object");
    if !root_object.get(key).is_some_and(Value::is_object) {
        root_object.insert(key.to_string(), json!({}));
    }
    root_object
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("field was normalized to object")
}

pub(crate) struct SupervisorCapabilityContext<'a> {
    pub(crate) connected: bool,
    pub(crate) ready: bool,
    pub(crate) instance_id: Option<String>,
    pub(crate) connection_id: Option<String>,
    pub(crate) process: &'a ProcessSnapshot,
    pub(crate) configured_launch_target: bool,
    pub(crate) cargo_available: bool,
    pub(crate) permissions: SupervisorPermissions,
    pub(crate) cargo_error: Option<CargoError>,
    pub(crate) host_error: Option<Value>,
}

pub(crate) fn merge_supervisor_capabilities(
    mut host: Value,
    context: SupervisorCapabilityContext<'_>,
) -> Value {
    let SupervisorCapabilityContext {
        connected,
        ready,
        instance_id,
        connection_id,
        process,
        configured_launch_target,
        cargo_available,
        permissions,
        cargo_error,
        host_error,
    } = context;
    if !host.is_object() {
        host = json!({});
    }
    let rebuild_allowed = permissions.cargo_check
        && permissions.cargo_build
        && permissions.process_stop
        && permissions.process_launch;
    let direct_launch_available = configured_launch_target
        && process.ownership != ProcessOwnership::External
        && !matches!(
            process.state,
            ProcessState::Running | ProcessState::Starting | ProcessState::Stopping
        );
    let managed = process.ownership == ProcessOwnership::Managed;
    let managed_running = managed
        && matches!(
            process.state,
            ProcessState::Running | ProcessState::Starting | ProcessState::Stopping
        );
    let rebuild_available = cargo_available && process.ownership != ProcessOwnership::External;

    {
        let object = host.as_object_mut().unwrap();
        object.insert("schema_version".to_string(), json!(2));
        object.insert("mode".to_string(), json!("supervised"));
        object.insert("connected".to_string(), json!(connected));
        object.insert("ready".to_string(), json!(ready));
        object.insert("instance_id".to_string(), json!(instance_id));
        object.insert("connection_id".to_string(), json!(connection_id));
    }

    {
        let permission_object = object_field(&mut host, "permissions");
        permission_object.insert(
            "build".to_string(),
            json!(permissions.cargo_check || permissions.cargo_build || permissions.cargo_test),
        );
        permission_object.insert("cargo_check".to_string(), json!(permissions.cargo_check));
        permission_object.insert("cargo_build".to_string(), json!(permissions.cargo_build));
        permission_object.insert("cargo_test".to_string(), json!(permissions.cargo_test));
        permission_object.insert(
            "process_launch".to_string(),
            json!(permissions.process_launch),
        );
        permission_object.insert("process_stop".to_string(), json!(permissions.process_stop));
        permission_object.insert(
            "process_restart".to_string(),
            json!(permissions.process_restart),
        );
    }

    {
        let runtime = object_field(&mut host, "runtime");
        runtime.insert(
            "launch".to_string(),
            capability(true, direct_launch_available, permissions.process_launch),
        );
        runtime.insert(
            "stop".to_string(),
            capability(true, managed_running, permissions.process_stop),
        );
        runtime.insert(
            "restart".to_string(),
            capability(true, managed, permissions.process_restart),
        );
        runtime.insert(
            "rebuild_restart".to_string(),
            capability(true, rebuild_available, rebuild_allowed),
        );
    }

    {
        let build = object_field(&mut host, "build");
        build.insert(
            "check".to_string(),
            capability(true, cargo_available, permissions.cargo_check),
        );
        build.insert(
            "build".to_string(),
            capability(true, cargo_available, permissions.cargo_build),
        );
        build.insert(
            "test".to_string(),
            capability(true, cargo_available, permissions.cargo_test),
        );
    }

    let supervisor = json!({
        "cargo": {
            "available": cargo_available,
            "initialization_error": cargo_error.map(|error| error.to_json()),
        },
        "process": process,
        "configured_launch_target": configured_launch_target,
        "rebuild_restart": {
            "implemented": true,
            "available": rebuild_available,
            "allowed": rebuild_allowed,
            "operational": rebuild_available && rebuild_allowed,
            "policy": "check-while-running -> stop -> build -> launch-cargo-artifact -> authenticate -> host-probe-ready",
        },
        "host_capability_error": host_error,
    });
    host.as_object_mut()
        .unwrap()
        .insert("supervisor".to_string(), supervisor);
    host
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProcessLogsParams {
    #[schemars(description = "Optional stream filter: stdout or stderr.")]
    pub stream: Option<String>,
    #[schemars(description = "Maximum newest log lines to return (default 200).")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProcessEvidenceParams {
    #[schemars(description = "Maximum newest stdout/stderr lines per stream (default 50).")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DevelopmentStatusParams {
    #[schemars(
        description = "Maximum newest stdout/stderr lines retained in failure evidence (default 50)."
    )]
    pub evidence_limit: Option<u32>,
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
    #[schemars(description = "Operation ID to check. Omit to list supervisor operations.")]
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
    rebuild: RebuildRestartCoordinator,
    permissions: SupervisorPermissions,
}

impl SupervisorToolServer {
    fn new(
        manager: ProcessManager,
        cargo: CargoExecutor,
        permissions: SupervisorPermissions,
    ) -> Self {
        let rebuild = RebuildRestartCoordinator::new(manager.clone(), cargo.clone(), permissions);
        Self {
            manager,
            cargo,
            rebuild,
            permissions,
        }
    }
}

#[tool_router(server_handler)]
impl SupervisorToolServer {
    #[tool(
        description = "Report the merged live capability contract from the Bevy host and persistent supervisor, including Cargo and process lifecycle availability."
    )]
    async fn capabilities(&self) -> String {
        let backend = self.manager.backend();
        let backend_status = backend.status();
        let mut host_error = None;
        let host = if backend_status.connected && backend_status.ready {
            match backend
                .call(McpCommand::Capabilities, Duration::from_secs(5))
                .await
            {
                Ok(McpResult::Success(value)) => value,
                Ok(McpResult::Error { code, message }) => {
                    host_error = Some(json!({ "error": code, "message": message }));
                    json!({})
                }
                Err(error) => {
                    host_error = Some(json!({ "error": error.code, "message": error.message }));
                    json!({})
                }
            }
        } else {
            host_error = Some(json!({
                "error": "GAME_UNAVAILABLE",
                "message": "Bevy host is not ready; host-only runtime capabilities are unavailable"
            }));
            json!({})
        };
        let process = self.manager.status().await;
        merge_supervisor_capabilities(
            host,
            SupervisorCapabilityContext {
                connected: backend_status.connected,
                ready: backend_status.ready,
                instance_id: backend_status.instance_id,
                connection_id: backend_status.connection_id,
                process: &process,
                configured_launch_target: self.manager.has_configured_launch_target(),
                cargo_available: self.cargo.available(),
                permissions: self.permissions,
                cargo_error: self.cargo.initialization_error(),
                host_error,
            },
        )
        .to_string()
    }

    #[tool(
        description = "Return one agent-oriented development diagnosis: current state, active operation, runtime/build identity, most recent failure with compiler/process evidence, and the recommended next tool/action."
    )]
    async fn development_status(
        &self,
        Parameters(params): Parameters<DevelopmentStatusParams>,
    ) -> String {
        let limit = params.evidence_limit.unwrap_or(50).clamp(1, 1000) as usize;
        let status = DevelopmentStatus::collect(
            &self.manager,
            &self.cargo,
            &self.rebuild,
            self.permissions,
            limit,
        )
        .await;
        serde_json::to_string(&status).unwrap()
    }

    #[tool(
        description = "Return managed/external process ownership plus process, transport, and Bevy-host readiness state."
    )]
    async fn process_status(&self) -> String {
        serde_json::to_string(&self.manager.status().await).unwrap()
    }

    #[tool(
        description = "Return process status plus bounded stdout/stderr tails for startup and crash diagnosis."
    )]
    async fn process_evidence(
        &self,
        Parameters(params): Parameters<ProcessEvidenceParams>,
    ) -> String {
        let limit = params.limit.unwrap_or(50).clamp(1, 1000) as usize;
        let status = self.manager.status().await;
        let stdout_tail = self.manager.logs(Some("stdout"), limit).unwrap_or_default();
        let stderr_tail = self.manager.logs(Some("stderr"), limit).unwrap_or_default();
        json!({
            "process": status,
            "stdout_tail": stdout_tail,
            "stderr_tail": stderr_tail,
        })
        .to_string()
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
            Ok(logs) => json!({ "logs": logs, "count": logs.len() }).to_string(),
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
        description = "Start the conservative autonomous development cycle: check while the current game remains live, stop only after check succeeds, build, launch Cargo's reported executable, authenticate, and wait for host readiness."
    )]
    async fn rebuild_restart(&self, Parameters(params): Parameters<CargoToolParams>) -> String {
        match self.rebuild.start(params.into_invocation()) {
            Ok(operation) => serde_json::to_string(&operation).unwrap(),
            Err(error) => error.to_json().to_string(),
        }
    }

    #[tool(
        description = "Read a supervisor Cargo or rebuild_restart operation by ID, or list all supervisor operations when no ID is supplied. Game operation IDs continue to route to the Bevy host."
    )]
    async fn operation_status(
        &self,
        Parameters(params): Parameters<OperationStatusParams>,
    ) -> String {
        if let Some(operation_id) = params.operation_id.as_deref() {
            if operation_id.starts_with("supervisor:rebuild_restart:") {
                return match self.rebuild.status(Some(operation_id)) {
                    Ok(mut operations) => serde_json::to_string(&operations.remove(0)).unwrap(),
                    Err(error) => error.to_json().to_string(),
                };
            }
            return match self.cargo.status(Some(operation_id)) {
                Ok(mut operations) => serde_json::to_string(&operations.remove(0)).unwrap(),
                Err(error) => error.to_json().to_string(),
            };
        }

        let cargo = match self.cargo.status(None) {
            Ok(operations) => operations,
            Err(error) => return error.to_json().to_string(),
        };
        let rebuild = match self.rebuild.status(None) {
            Ok(operations) => operations,
            Err(error) => return error.to_json().to_string(),
        };
        let mut operations: Vec<Value> = cargo
            .into_iter()
            .filter_map(|operation| serde_json::to_value(operation).ok())
            .collect();
        operations.extend(
            rebuild
                .into_iter()
                .filter_map(|operation| serde_json::to_value(operation).ok()),
        );
        operations.sort_by_key(|operation| {
            operation
                .get("created_unix_ms")
                .and_then(Value::as_u64)
                .unwrap_or_default()
        });
        json!({
            "count": operations.len(),
            "operations": operations,
        })
        .to_string()
    }

    #[tool(
        description = "Cancel a supervisor Cargo or rebuild_restart operation. Cargo process trees are terminated; lifecycle-stage cancellation is applied at the next safe boundary. Game operation IDs continue to route to the Bevy host."
    )]
    async fn operation_cancel(
        &self,
        Parameters(params): Parameters<OperationCancelParams>,
    ) -> String {
        if params
            .operation_id
            .starts_with("supervisor:rebuild_restart:")
        {
            return match self.rebuild.cancel(&params.operation_id).await {
                Ok(operation) => serde_json::to_string(&operation).unwrap(),
                Err(error) => error.to_json().to_string(),
            };
        }
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

#[cfg(test)]
mod permission_tests {
    use super::*;
    use crate::{CargoExecutorConfig, ProcessManagerConfig, SupervisorTransport};

    async fn read_only_server() -> (SupervisorToolServer, ProcessManager) {
        let transport = SupervisorTransport::bind("permission-tool-test", "permission-tool-secret")
            .await
            .unwrap();
        let manager = ProcessManager::new(
            transport.backend(),
            transport.address(),
            "permission-tool-secret",
            ProcessManagerConfig::default(),
        );
        let cargo = CargoExecutor::initialize(CargoExecutorConfig {
            permissions: SupervisorPermissions::read_only(),
            ..CargoExecutorConfig::new(env!("CARGO_MANIFEST_DIR"))
        })
        .await;
        let server = SupervisorToolServer::new(
            manager.clone(),
            cargo,
            SupervisorPermissions::read_only(),
        );
        (server, manager)
    }

    #[tokio::test]
    async fn lifecycle_tools_deny_before_touching_process_state() {
        let (server, manager) = read_only_server().await;
        let before = manager.status().await;

        for (operation, response) in [
            ("process_launch", server.process_launch().await),
            ("process_stop", server.process_stop().await),
            ("process_restart", server.process_restart().await),
        ] {
            let value: Value = serde_json::from_str(&response).unwrap();
            assert_eq!(value["error"], "SUPERVISOR_PERMISSION_DENIED");
            assert!(
                value["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(operation))
            );
            let after = manager.status().await;
            assert_eq!(after.state, before.state);
            assert_eq!(after.ownership, before.ownership);
            assert_eq!(after.pid, before.pid);
            assert_eq!(after.instance_id, before.instance_id);
            assert_eq!(after.connection_id, before.connection_id);
        }
    }
}
