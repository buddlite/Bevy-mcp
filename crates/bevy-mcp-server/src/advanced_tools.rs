use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bevy_mcp_core::advanced::{
    AdvancedEntityQuery, AdvancedRequest, CaptureOptions, CaptureRect, QueryCondition,
    encode_advanced_request,
};
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::entity_handle::EntityHandle;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ServerInfo,
    Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::Value;

use crate::tools::{BevyMcpServer, BevyMcpState};

#[derive(Clone)]
struct AdvancedMcpState {
    ingress: McpIngressQueue,
    results: McpResultQueue,
    connected: Arc<std::sync::atomic::AtomicBool>,
    next_id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, McpResult>>>,
}

impl AdvancedMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            ingress: state.ingress.clone(),
            results: state.results.clone(),
            connected: state.connected.clone(),
            next_id: Arc::new(AtomicU64::new(1 << 63)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn call(&self, request: AdvancedRequest) -> String {
        if !self.connected.load(Ordering::Relaxed) {
            return serde_json::json!({
                "error": "RUNTIME_NOT_RUNNING",
                "message": "No embedded Bevy application is connected."
            })
            .to_string();
        }

        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let operation_id = match encode_advanced_request(&request) {
            Ok(value) => value,
            Err(error) => {
                return serde_json::json!({
                    "error": "ADVANCED_REQUEST_ENCODING_FAILED",
                    "message": error.to_string(),
                })
                .to_string();
            }
        };
        self.ingress.push(
            request_id,
            McpCommand::OperationStatus {
                operation_id: Some(operation_id),
            },
        );

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
        loop {
            if let Some(result) = self.pending.lock().unwrap().remove(&request_id) {
                return format_result(result);
            }

            if tokio::time::Instant::now() >= deadline {
                return serde_json::json!({
                    "error": "TIMEOUT",
                    "message": "Bevy app did not respond within 15 seconds"
                })
                .to_string();
            }

            for response in self.results.drain() {
                if response.request_id == request_id {
                    return format_result(response.result);
                }
                self.pending
                    .lock()
                    .unwrap()
                    .insert(response.request_id, response.result);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    }
}

fn format_result(result: McpResult) -> String {
    match result {
        McpResult::Success(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into()),
        McpResult::Error { code, message } => serde_json::json!({
            "error": code,
            "message": message,
        })
        .to_string(),
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CropParams {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CaptureViewportParams {
    #[schemars(description = "Optional camera entity handle. Omit for the primary window.")]
    pub camera: Option<String>,
    #[schemars(description = "Optional pixel crop rectangle applied after GPU capture.")]
    pub crop: Option<CropParams>,
    #[schemars(description = "Capture the registered dedicated UI render target only.")]
    pub ui_only: Option<bool>,
    #[schemars(description = "Optional stable filename stem for the PNG capture.")]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SinceFrameParams {
    #[schemars(description = "Return changes strictly newer than this completed MCP frame.")]
    pub frame: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntityChangesParams {
    pub frame: u64,
    #[schemars(description = "Optional entity handle. Omit to return changes for all entities.")]
    pub entity: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComponentChangesParams {
    pub frame: u64,
    #[schemars(description = "Optional component type name, short or fully-qualified.")]
    pub component: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResourceChangesParams {
    pub frame: u64,
    #[schemars(description = "Optional resource type name, short or fully-qualified.")]
    pub resource: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScheduleParams {
    pub schedule: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SystemListParams {
    pub schedule: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SystemInspectParams {
    pub system: String,
    pub schedule: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StateGetParams {
    #[schemars(description = "Registered MCP state name. Omit to list all registered states and values.")]
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StateTransitionParams {
    pub state: String,
    #[schemars(description = "Serialized next state value.")]
    pub value: Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PredicateParams {
    #[schemars(description = "Operator: eq, ne, lt, lte, gt, gte, contains.")]
    pub op: String,
    pub value: Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AdvancedEntityQueryParams {
    pub with_components: Option<Vec<String>>,
    pub without_components: Option<Vec<String>>,
    #[schemars(description = "Reflected components to include in each result.")]
    pub include: Option<Vec<String>>,
    #[schemars(description = "Field predicates keyed by Component.field.path.")]
    pub predicates: Option<HashMap<String, PredicateParams>>,
    #[schemars(description = "Components that must have changed in the most recently completed frame.")]
    pub changed: Option<Vec<String>>,
    #[schemars(description = "Components that the immediate parent must have.")]
    pub parent_has: Option<Vec<String>>,
    #[schemars(description = "For each component listed, at least one immediate child must have it.")]
    pub child_has: Option<Vec<String>>,
    pub name_contains: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SemanticActionInvokeParams {
    pub action: String,
    #[schemars(description = "Game-defined JSON arguments for the semantic action.")]
    pub args: Value,
}

#[derive(Clone)]
pub struct AdvancedBevyMcpServer {
    state: AdvancedMcpState,
}

impl AdvancedBevyMcpServer {
    fn new(state: AdvancedMcpState) -> Self {
        Self { state }
    }
}

#[tool_router(server_handler)]
impl AdvancedBevyMcpServer {
    #[tool(description = "Capture the primary window, a camera render target, a crop, or a registered UI-only render target. Returns a PNG path after capture completes.")]
    async fn capture_viewport(
        &self,
        Parameters(params): Parameters<CaptureViewportParams>,
    ) -> String {
        let camera = match params.camera {
            Some(value) => match EntityHandle::from_uri(&value) {
                Ok(handle) => Some(handle),
                Err(message) => return serde_json::json!({ "error": "INVALID_ENTITY_HANDLE", "message": message }).to_string(),
            },
            None => None,
        };
        self.state
            .call(AdvancedRequest::Capture {
                options: CaptureOptions {
                    camera,
                    crop: params.crop.map(|crop| CaptureRect {
                        x: crop.x,
                        y: crop.y,
                        width: crop.width,
                        height: crop.height,
                    }),
                    ui_only: params.ui_only.unwrap_or(false),
                    name: params.name,
                },
            })
            .await
    }

    #[tool(description = "Return compact spawned/despawned entity, component, and resource deltas newer than a frame.")]
    async fn changes_since(&self, Parameters(params): Parameters<SinceFrameParams>) -> String {
        self.state.call(AdvancedRequest::ChangesSince { frame: params.frame }).await
    }

    #[tool(description = "Return entity lifecycle and component changes newer than a frame, optionally for one entity.")]
    async fn entity_changes(&self, Parameters(params): Parameters<EntityChangesParams>) -> String {
        let entity = match params.entity {
            Some(value) => match EntityHandle::from_uri(&value) {
                Ok(handle) => Some(handle),
                Err(message) => return serde_json::json!({ "error": "INVALID_ENTITY_HANDLE", "message": message }).to_string(),
            },
            None => None,
        };
        self.state
            .call(AdvancedRequest::EntityChanges { frame: params.frame, entity })
            .await
    }

    #[tool(description = "Return added, changed, and removed component records newer than a frame.")]
    async fn component_changes(&self, Parameters(params): Parameters<ComponentChangesParams>) -> String {
        self.state
            .call(AdvancedRequest::ComponentChanges {
                frame: params.frame,
                component: params.component,
            })
            .await
    }

    #[tool(description = "Return added, changed, and removed resource records newer than a frame.")]
    async fn resource_changes(&self, Parameters(params): Parameters<ResourceChangesParams>) -> String {
        self.state
            .call(AdvancedRequest::ResourceChanges {
                frame: params.frame,
                resource: params.resource,
            })
            .await
    }

    #[tool(description = "List Bevy schedules with system counts and initialization state.")]
    async fn schedule_list(&self) -> String {
        self.state.call(AdvancedRequest::ScheduleList).await
    }

    #[tool(description = "Inspect a Bevy schedule, including systems, run-condition counts, and access conflicts.")]
    async fn schedule_inspect(&self, Parameters(params): Parameters<ScheduleParams>) -> String {
        self.state
            .call(AdvancedRequest::ScheduleInspect { schedule: params.schedule })
            .await
    }

    #[tool(description = "List Bevy systems, optionally scoped to a schedule.")]
    async fn system_list(&self, Parameters(params): Parameters<SystemListParams>) -> String {
        self.state.call(AdvancedRequest::SystemList { schedule: params.schedule }).await
    }

    #[tool(description = "Inspect one Bevy system across schedules, including last-run tick and run-condition count.")]
    async fn system_inspect(&self, Parameters(params): Parameters<SystemInspectParams>) -> String {
        self.state
            .call(AdvancedRequest::SystemInspect {
                system: params.system,
                schedule: params.schedule,
            })
            .await
    }

    #[tool(description = "Return explicitly instrumented per-system timing statistics.")]
    async fn system_timings(&self, Parameters(params): Parameters<SystemListParams>) -> String {
        self.state.call(AdvancedRequest::SystemTimings { schedule: params.schedule }).await
    }

    #[tool(description = "Read one registered typed Bevy state, or list every MCP-registered state and current value.")]
    async fn state_get(&self, Parameters(params): Parameters<StateGetParams>) -> String {
        self.state.call(AdvancedRequest::StateGet { state: params.state }).await
    }

    #[tool(description = "Queue a transition for an MCP-registered typed Bevy state. Requires write permission.")]
    async fn state_transition(&self, Parameters(params): Parameters<StateTransitionParams>) -> String {
        self.state
            .call(AdvancedRequest::StateTransition {
                state: params.state,
                value: params.value,
            })
            .await
    }

    #[tool(description = "Agent-oriented ECS query with field predicates, change filters, hierarchy relationships, name matching, and reflected includes.")]
    async fn entity_query_advanced(
        &self,
        Parameters(params): Parameters<AdvancedEntityQueryParams>,
    ) -> String {
        let predicates = params
            .predicates
            .unwrap_or_default()
            .into_iter()
            .map(|(path, predicate)| {
                (
                    path,
                    QueryCondition {
                        op: predicate.op,
                        value: predicate.value,
                    },
                )
            })
            .collect();
        self.state
            .call(AdvancedRequest::EntityQuery {
                query: AdvancedEntityQuery {
                    with_components: params.with_components.unwrap_or_default(),
                    without_components: params.without_components.unwrap_or_default(),
                    include: params.include.unwrap_or_default(),
                    predicates,
                    changed: params.changed.unwrap_or_default(),
                    parent_has: params.parent_has.unwrap_or_default(),
                    child_has: params.child_has.unwrap_or_default(),
                    name_contains: params.name_contains,
                    limit: params.limit.unwrap_or(100),
                },
            })
            .await
    }

    #[tool(description = "List game-specific semantic actions registered by the Bevy application.")]
    async fn semantic_action_list(&self) -> String {
        self.state.call(AdvancedRequest::SemanticActionList).await
    }

    #[tool(description = "Invoke a game-specific semantic action with JSON arguments. Requires write permission.")]
    async fn semantic_action_invoke(
        &self,
        Parameters(params): Parameters<SemanticActionInvokeParams>,
    ) -> String {
        self.state
            .call(AdvancedRequest::SemanticActionInvoke {
                action: params.action,
                args: params.args,
            })
            .await
    }
}

/// Combines the legacy tool server and the advanced agent tool server while serializing
/// queue consumers so correlated responses cannot be drained by the wrong router.
#[derive(Clone)]
pub struct UnifiedBevyMcpServer {
    legacy: BevyMcpServer,
    advanced: AdvancedBevyMcpServer,
    call_gate: Arc<tokio::sync::Mutex<()>>,
}

impl UnifiedBevyMcpServer {
    pub fn new(state: BevyMcpState) -> Self {
        let advanced = AdvancedBevyMcpServer::new(AdvancedMcpState::from_base(&state));
        Self {
            legacy: BevyMcpServer::new(state),
            advanced,
            call_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl ServerHandler for UnifiedBevyMcpServer {
    fn get_info(&self) -> ServerInfo {
        self.legacy.get_info()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let _guard = self.call_gate.lock().await;
        if self.advanced.get_tool(request.name.as_ref()).is_some() {
            self.advanced.call_tool(request, context).await
        } else {
            self.legacy.call_tool(request, context).await
        }
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut legacy = self.legacy.list_tools(request.clone(), context.clone()).await?;
        let advanced = self.advanced.list_tools(request, context).await?;
        legacy.tools.extend(advanced.tools);
        Ok(legacy)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.advanced.get_tool(name).or_else(|| self.legacy.get_tool(name))
    }
}
