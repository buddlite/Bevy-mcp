use std::collections::HashMap;

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

use crate::response_dispatcher::McpResponseDispatcher;
use crate::tools::{BevyMcpServer, BevyMcpState};

#[derive(Clone)]
struct AdvancedMcpState {
    dispatcher: McpResponseDispatcher,
}

impl AdvancedMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            dispatcher: state.dispatcher.clone(),
        }
    }

    async fn call(&self, request: AdvancedRequest) -> String {
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
        self.dispatcher
            .call(
                McpCommand::OperationStatus {
                    operation_id: Some(operation_id),
                },
                std::time::Duration::from_secs(15),
            )
            .await
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
pub struct WriterSearchParams {
    pub name: String,
    pub schedule: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TrackingConfigParams {
    #[schemars(
        description = "Tracking mode: full or scoped. Scoped only snapshots subscribed component/resource ticks."
    )]
    pub mode: Option<String>,
    pub history_frames: Option<usize>,
    pub components: Option<Vec<String>>,
    pub resources: Option<Vec<String>>,
    pub exclude_components: Option<Vec<String>>,
    pub exclude_resources: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StateGetParams {
    #[schemars(
        description = "Registered MCP state name. Omit to list all registered states and values."
    )]
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
    #[schemars(
        description = "Components that must have changed in the most recently completed frame."
    )]
    pub changed: Option<Vec<String>>,
    #[schemars(description = "Components that the immediate parent must have.")]
    pub parent_has: Option<Vec<String>>,
    #[schemars(
        description = "For each component listed, at least one immediate child must have it."
    )]
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
    #[tool(
        description = "Capture the primary window, a camera render target, a crop, or a registered UI-only render target. Returns a PNG path after capture completes."
    )]
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

    #[tool(
        description = "Return compact spawned/despawned entity, component, and resource deltas newer than a frame."
    )]
    async fn changes_since(&self, Parameters(params): Parameters<SinceFrameParams>) -> String {
        self.state
            .call(AdvancedRequest::ChangesSince {
                frame: params.frame,
            })
            .await
    }

    #[tool(
        description = "Return entity lifecycle and component changes newer than a frame, optionally for one entity."
    )]
    async fn entity_changes(&self, Parameters(params): Parameters<EntityChangesParams>) -> String {
        let entity = match params.entity {
            Some(value) => match EntityHandle::from_uri(&value) {
                Ok(handle) => Some(handle),
                Err(message) => return serde_json::json!({ "error": "INVALID_ENTITY_HANDLE", "message": message }).to_string(),
            },
            None => None,
        };
        self.state
            .call(AdvancedRequest::EntityChanges {
                frame: params.frame,
                entity,
            })
            .await
    }

    #[tool(
        description = "Return added, changed, and removed component records newer than a frame."
    )]
    async fn component_changes(
        &self,
        Parameters(params): Parameters<ComponentChangesParams>,
    ) -> String {
        self.state
            .call(AdvancedRequest::ComponentChanges {
                frame: params.frame,
                component: params.component,
            })
            .await
    }

    #[tool(description = "Return added, changed, and removed resource records newer than a frame.")]
    async fn resource_changes(
        &self,
        Parameters(params): Parameters<ResourceChangesParams>,
    ) -> String {
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

    #[tool(
        description = "Inspect a Bevy schedule, including systems, run-condition counts, and access conflicts."
    )]
    async fn schedule_inspect(&self, Parameters(params): Parameters<ScheduleParams>) -> String {
        self.state
            .call(AdvancedRequest::ScheduleInspect {
                schedule: params.schedule,
            })
            .await
    }

    #[tool(description = "List Bevy systems, optionally scoped to a schedule.")]
    async fn system_list(&self, Parameters(params): Parameters<SystemListParams>) -> String {
        self.state
            .call(AdvancedRequest::SystemList {
                schedule: params.schedule,
            })
            .await
    }

    #[tool(
        description = "Inspect one Bevy system across schedules, including last-run tick and run-condition count."
    )]
    async fn system_inspect(&self, Parameters(params): Parameters<SystemInspectParams>) -> String {
        self.state
            .call(AdvancedRequest::SystemInspect {
                system: params.system,
                schedule: params.schedule,
            })
            .await
    }

    #[tool(
        description = "Inspect the declared ECS read/write access of a system, including resources and unbounded World access."
    )]
    async fn system_access(&self, Parameters(params): Parameters<SystemInspectParams>) -> String {
        self.state
            .call(AdvancedRequest::SystemAccess {
                system: params.system,
                schedule: params.schedule,
            })
            .await
    }

    #[tool(
        description = "Find initialized Bevy systems that can write a component. Useful for runtime-to-code causal debugging."
    )]
    async fn component_writers(
        &self,
        Parameters(params): Parameters<WriterSearchParams>,
    ) -> String {
        self.state
            .call(AdvancedRequest::ComponentWriters {
                component: params.name,
                schedule: params.schedule,
            })
            .await
    }

    #[tool(description = "Find initialized Bevy systems that can write a resource.")]
    async fn resource_writers(&self, Parameters(params): Parameters<WriterSearchParams>) -> String {
        self.state
            .call(AdvancedRequest::ResourceWriters {
                resource: params.name,
                schedule: params.schedule,
            })
            .await
    }

    #[tool(
        description = "Configure world-change tracking. Use scoped mode to reduce per-frame component/resource tick snapshot cost."
    )]
    async fn tracking_config(
        &self,
        Parameters(params): Parameters<TrackingConfigParams>,
    ) -> String {
        self.state
            .call(AdvancedRequest::TrackingConfig {
                mode: params.mode,
                history_frames: params.history_frames,
                components: params.components,
                resources: params.resources,
                exclude_components: params.exclude_components,
                exclude_resources: params.exclude_resources,
            })
            .await
    }

    #[tool(
        description = "Inspect current change-tracking mode, history, explicit scopes, and debugger-derived subscriptions."
    )]
    async fn tracking_status(&self) -> String {
        self.state.call(AdvancedRequest::TrackingStatus).await
    }

    #[tool(description = "Return explicitly instrumented per-system timing statistics.")]
    async fn system_timings(&self, Parameters(params): Parameters<SystemListParams>) -> String {
        self.state
            .call(AdvancedRequest::SystemTimings {
                schedule: params.schedule,
            })
            .await
    }

    #[tool(
        description = "Read one registered typed Bevy state, or list every MCP-registered state and current value."
    )]
    async fn state_get(&self, Parameters(params): Parameters<StateGetParams>) -> String {
        self.state
            .call(AdvancedRequest::StateGet {
                state: params.state,
            })
            .await
    }

    #[tool(
        description = "Queue a transition for an MCP-registered typed Bevy state. Requires write permission."
    )]
    async fn state_transition(
        &self,
        Parameters(params): Parameters<StateTransitionParams>,
    ) -> String {
        self.state
            .call(AdvancedRequest::StateTransition {
                state: params.state,
                value: params.value,
            })
            .await
    }

    #[tool(
        description = "Agent-oriented ECS query with field predicates, change filters, hierarchy relationships, name matching, and reflected includes."
    )]
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

    #[tool(
        description = "Invoke a game-specific semantic action with JSON arguments. Requires write permission."
    )]
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

/// Combines legacy and advanced tools. Responses are correlated by the shared dispatcher,
/// so independent MCP calls can execute concurrently.
#[derive(Clone)]
pub struct UnifiedBevyMcpServer {
    legacy: BevyMcpServer,
    advanced: AdvancedBevyMcpServer,
}

impl UnifiedBevyMcpServer {
    pub fn new(state: BevyMcpState) -> Self {
        let advanced = AdvancedBevyMcpServer::new(AdvancedMcpState::from_base(&state));
        Self {
            legacy: BevyMcpServer::new(state),
            advanced,
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
        let mut legacy = self
            .legacy
            .list_tools(request.clone(), context.clone())
            .await?;
        let advanced = self.advanced.list_tools(request, context).await?;
        legacy.tools.extend(advanced.tools);
        Ok(legacy)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.advanced
            .get_tool(name)
            .or_else(|| self.legacy.get_tool(name))
    }
}
