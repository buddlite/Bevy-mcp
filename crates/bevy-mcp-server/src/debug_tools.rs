use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bevy_mcp_core::advanced::{AdvancedEntityQuery, QueryCondition};
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::debug::{
    DebugCondition, DebugPlaytestPlan, DebugPlaytestStep, DebugRequest, EvidenceOptions,
    WatchpointSpec, encode_debug_request,
};
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

use crate::advanced_tools::{AdvancedEntityQueryParams, UnifiedBevyMcpServer};
use crate::tools::BevyMcpState;

#[derive(Clone)]
struct DebugMcpState {
    ingress: McpIngressQueue,
    results: McpResultQueue,
    connected: Arc<std::sync::atomic::AtomicBool>,
    next_id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, McpResult>>>,
}

impl DebugMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            ingress: state.ingress.clone(),
            results: state.results.clone(),
            connected: state.connected.clone(),
            next_id: Arc::new(AtomicU64::new(1 << 62)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn call(&self, request: DebugRequest) -> String {
        if !self.connected.load(Ordering::Relaxed) {
            return serde_json::json!({
                "error": "RUNTIME_NOT_RUNNING",
                "message": "No embedded Bevy application is connected."
            })
            .to_string();
        }

        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let operation_id = match encode_debug_request(&request) {
            Ok(value) => value,
            Err(error) => {
                return serde_json::json!({
                    "error": "DEBUG_REQUEST_ENCODING_FAILED",
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

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            if let Some(result) = self.pending.lock().unwrap().remove(&request_id) {
                return format_result(result);
            }
            if tokio::time::Instant::now() >= deadline {
                return serde_json::json!({
                    "error": "TIMEOUT",
                    "message": "Bevy app did not respond within 5 seconds"
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
        McpResult::Success(value) => {
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into())
        }
        McpResult::Error { code, message } => serde_json::json!({
            "error": code,
            "message": message,
        })
        .to_string(),
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EvidenceParams {
    #[schemars(description = "How many completed frames of ECS/resource deltas to attach (default 120).")]
    pub changes_frames: Option<u64>,
    #[schemars(description = "Maximum recent log entries to attach (default 50).")]
    pub logs_limit: Option<u32>,
    #[schemars(description = "Maximum captured ECS/game events to attach (default 50).")]
    pub events_limit: Option<u32>,
    pub include_states: Option<bool>,
    pub include_system_timings: Option<bool>,
    #[schemars(description = "Capture the primary window when evidence is created (default true).")]
    pub screenshot: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConditionParams {
    #[schemars(description = "Condition kind: entity_exists, query_count, entity_field, resource_field, state_equals, log_contains, change_occurred, or frame_at_least.")]
    pub kind: String,
    #[schemars(description = "Entity handle for entity_exists/entity_field/change_occurred.")]
    pub entity: Option<String>,
    #[schemars(description = "Advanced query for query_count.")]
    pub query: Option<AdvancedEntityQueryParams>,
    #[schemars(description = "Comparison operator for query_count/entity_field/resource_field: eq, ne, lt, lte, gt, gte, contains.")]
    pub op: Option<String>,
    #[schemars(description = "Expected comparison value, or exact state value for state_equals.")]
    pub value: Option<Value>,
    #[schemars(description = "Component for entity_field/change_occurred.")]
    pub component: Option<String>,
    #[schemars(description = "Dot-separated reflected field path for entity_field/resource_field. Empty string compares the whole value.")]
    pub field: Option<String>,
    #[schemars(description = "Resource for resource_field/change_occurred.")]
    pub resource: Option<String>,
    #[schemars(description = "Registered MCP state name for state_equals.")]
    pub state: Option<String>,
    #[schemars(description = "Optional log level for log_contains.")]
    pub level: Option<String>,
    #[schemars(description = "Substring for log_contains.")]
    pub text: Option<String>,
    #[schemars(description = "Target frame for frame_at_least.")]
    pub frame: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WatchpointAddParams {
    pub name: String,
    pub condition: ConditionParams,
    #[schemars(description = "Pause the MCP runtime on the first frame where the condition becomes true.")]
    pub pause_on_trigger: Option<bool>,
    #[schemars(description = "Disable after the first trigger (default true).")]
    pub once: Option<bool>,
    pub evidence: Option<EvidenceParams>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IdParams {
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlaytestStepParams {
    #[schemars(description = "Step type: semantic_action, state_transition, key, step_frames, wait, assert, or capture.")]
    pub r#type: String,
    pub action: Option<String>,
    pub args: Option<Value>,
    pub state: Option<String>,
    pub value: Option<Value>,
    pub key: Option<String>,
    pub pressed: Option<bool>,
    pub frames: Option<u32>,
    pub condition: Option<ConditionParams>,
    pub timeout_frames: Option<u32>,
    pub message: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlaytestStartParams {
    pub name: String,
    pub steps: Vec<PlaytestStepParams>,
    #[schemars(description = "Pause the game automatically when a step fails (default true).")]
    pub pause_on_failure: Option<bool>,
    pub evidence: Option<EvidenceParams>,
}

fn evidence_from_params(params: Option<EvidenceParams>) -> EvidenceOptions {
    let defaults = EvidenceOptions::default();
    match params {
        Some(params) => EvidenceOptions {
            changes_frames: params.changes_frames.unwrap_or(defaults.changes_frames),
            logs_limit: params.logs_limit.unwrap_or(defaults.logs_limit),
            events_limit: params.events_limit.unwrap_or(defaults.events_limit),
            include_states: params.include_states.unwrap_or(defaults.include_states),
            include_system_timings: params
                .include_system_timings
                .unwrap_or(defaults.include_system_timings),
            screenshot: params.screenshot.unwrap_or(defaults.screenshot),
        },
        None => defaults,
    }
}

fn query_from_params(params: AdvancedEntityQueryParams) -> AdvancedEntityQuery {
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
    AdvancedEntityQuery {
        with_components: params.with_components.unwrap_or_default(),
        without_components: params.without_components.unwrap_or_default(),
        include: params.include.unwrap_or_default(),
        predicates,
        changed: params.changed.unwrap_or_default(),
        parent_has: params.parent_has.unwrap_or_default(),
        child_has: params.child_has.unwrap_or_default(),
        name_contains: params.name_contains,
        limit: params.limit.unwrap_or(10_000),
    }
}

fn condition_from_params(params: ConditionParams) -> Result<DebugCondition, String> {
    let kind = params.kind.to_lowercase();
    match kind.as_str() {
        "entity_exists" => Ok(DebugCondition::EntityExists {
            entity: parse_entity(required(params.entity, "entity")?)?,
        }),
        "query_count" => Ok(DebugCondition::QueryCount {
            query: query_from_params(required(params.query, "query")?),
            condition: QueryCondition {
                op: required(params.op, "op")?,
                value: required(params.value, "value")?,
            },
        }),
        "entity_field" => Ok(DebugCondition::EntityField {
            entity: parse_entity(required(params.entity, "entity")?)?,
            component: required(params.component, "component")?,
            field: params.field.unwrap_or_default(),
            condition: QueryCondition {
                op: required(params.op, "op")?,
                value: required(params.value, "value")?,
            },
        }),
        "resource_field" => Ok(DebugCondition::ResourceField {
            resource: required(params.resource, "resource")?,
            field: params.field.unwrap_or_default(),
            condition: QueryCondition {
                op: required(params.op, "op")?,
                value: required(params.value, "value")?,
            },
        }),
        "state_equals" => Ok(DebugCondition::StateEquals {
            state: required(params.state, "state")?,
            value: required(params.value, "value")?,
        }),
        "log_contains" => Ok(DebugCondition::LogContains {
            level: params.level,
            text: required(params.text, "text")?,
        }),
        "change_occurred" => Ok(DebugCondition::ChangeOccurred {
            entity: params.entity.map(parse_entity).transpose()?,
            component: params.component,
            resource: params.resource,
        }),
        "frame_at_least" => Ok(DebugCondition::FrameAtLeast {
            frame: required(params.frame, "frame")?,
        }),
        other => Err(format!("Unknown condition kind '{other}'")),
    }
}

fn step_from_params(params: PlaytestStepParams) -> Result<DebugPlaytestStep, String> {
    match params.r#type.to_lowercase().as_str() {
        "semantic_action" => Ok(DebugPlaytestStep::SemanticAction {
            action: required(params.action, "action")?,
            args: params.args.unwrap_or(Value::Null),
        }),
        "state_transition" => Ok(DebugPlaytestStep::StateTransition {
            state: required(params.state, "state")?,
            value: required(params.value, "value")?,
        }),
        "key" => Ok(DebugPlaytestStep::Key {
            key: required(params.key, "key")?,
            pressed: params.pressed.unwrap_or(true),
        }),
        "step_frames" => Ok(DebugPlaytestStep::StepFrames {
            frames: required(params.frames, "frames")?,
        }),
        "wait" => Ok(DebugPlaytestStep::Wait {
            condition: condition_from_params(required(params.condition, "condition")?)?,
            timeout_frames: params.timeout_frames.unwrap_or(600),
        }),
        "assert" => Ok(DebugPlaytestStep::Assert {
            condition: condition_from_params(required(params.condition, "condition")?)?,
            message: params.message,
        }),
        "capture" => Ok(DebugPlaytestStep::Capture { name: params.name }),
        other => Err(format!("Unknown playtest step type '{other}'")),
    }
}

fn required<T>(value: Option<T>, field: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("Missing required field '{field}'"))
}

fn parse_entity(value: String) -> Result<EntityHandle, String> {
    EntityHandle::from_uri(&value)
}

#[derive(Clone)]
pub struct DebugBevyMcpServer {
    state: DebugMcpState,
}

impl DebugBevyMcpServer {
    fn new(state: DebugMcpState) -> Self {
        Self { state }
    }
}

#[tool_router(server_handler)]
impl DebugBevyMcpServer {
    #[tool(description = "Add a frame-evaluated debugger watchpoint. It can pause on a condition edge and automatically attach recent world deltas, logs, events, states, system timings, and a screenshot.")]
    async fn watchpoint_add(
        &self,
        Parameters(params): Parameters<WatchpointAddParams>,
    ) -> String {
        let condition = match condition_from_params(params.condition) {
            Ok(condition) => condition,
            Err(message) => {
                return serde_json::json!({ "error": "INVALID_CONDITION", "message": message })
                    .to_string();
            }
        };
        self.state
            .call(DebugRequest::WatchpointAdd {
                spec: WatchpointSpec {
                    name: params.name,
                    condition,
                    pause_on_trigger: params.pause_on_trigger.unwrap_or(false),
                    once: params.once.unwrap_or(true),
                    evidence: evidence_from_params(params.evidence),
                },
            })
            .await
    }

    #[tool(description = "List debugger watchpoints including trigger state, last evaluation, and resolved evidence/screenshot status.")]
    async fn watchpoint_list(&self) -> String {
        self.state.call(DebugRequest::WatchpointList).await
    }

    #[tool(description = "Remove one debugger watchpoint.")]
    async fn watchpoint_remove(&self, Parameters(params): Parameters<IdParams>) -> String {
        self.state
            .call(DebugRequest::WatchpointRemove { id: params.id })
            .await
    }

    #[tool(description = "Remove all debugger watchpoints.")]
    async fn watchpoint_clear(&self) -> String {
        self.state.call(DebugRequest::WatchpointClear).await
    }

    #[tool(description = "Start a non-blocking, frame-driven agent playtest. Steps can invoke semantic actions, transition states, inject keys, wait, assert, step frames, and capture screenshots. Failures automatically collect an evidence bundle.")]
    async fn playtest_start(
        &self,
        Parameters(params): Parameters<PlaytestStartParams>,
    ) -> String {
        let mut steps = Vec::with_capacity(params.steps.len());
        for (index, step) in params.steps.into_iter().enumerate() {
            match step_from_params(step) {
                Ok(step) => steps.push(step),
                Err(message) => {
                    return serde_json::json!({
                        "error": "INVALID_PLAYTEST_STEP",
                        "message": format!("Step {index}: {message}"),
                    })
                    .to_string();
                }
            }
        }
        self.state
            .call(DebugRequest::PlaytestStart {
                plan: DebugPlaytestPlan {
                    name: params.name,
                    steps,
                    pause_on_failure: params.pause_on_failure.unwrap_or(true),
                    evidence: evidence_from_params(params.evidence),
                },
            })
            .await
    }

    #[tool(description = "Read a playtest's live progress, step results, failure details, evidence bundle, and screenshot completion status.")]
    async fn playtest_status(&self, Parameters(params): Parameters<IdParams>) -> String {
        self.state
            .call(DebugRequest::PlaytestStatus { id: params.id })
            .await
    }

    #[tool(description = "List current and completed agent playtests.")]
    async fn playtest_list(&self) -> String {
        self.state.call(DebugRequest::PlaytestList).await
    }

    #[tool(description = "Cancel a running agent playtest.")]
    async fn playtest_cancel(&self, Parameters(params): Parameters<IdParams>) -> String {
        self.state
            .call(DebugRequest::PlaytestCancel { id: params.id })
            .await
    }
}

/// Top-level MCP server exposing legacy, advanced, and debugger/playtest tools while ensuring
/// only one tool call drains the shared response queue at a time.
#[derive(Clone)]
pub struct AgentBevyMcpServer {
    base: UnifiedBevyMcpServer,
    debug: DebugBevyMcpServer,
    call_gate: Arc<tokio::sync::Mutex<()>>,
}

impl AgentBevyMcpServer {
    pub fn new(state: BevyMcpState) -> Self {
        let debug = DebugBevyMcpServer::new(DebugMcpState::from_base(&state));
        Self {
            base: UnifiedBevyMcpServer::new(state),
            debug,
            call_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl ServerHandler for AgentBevyMcpServer {
    fn get_info(&self) -> ServerInfo {
        self.base.get_info()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let _guard = self.call_gate.lock().await;
        if self.debug.get_tool(request.name.as_ref()).is_some() {
            self.debug.call_tool(request, context).await
        } else {
            self.base.call_tool(request, context).await
        }
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut base = self.base.list_tools(request.clone(), context.clone()).await?;
        let debug = self.debug.list_tools(request, context).await?;
        base.tools.extend(debug.tools);
        Ok(base)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.debug.get_tool(name).or_else(|| self.base.get_tool(name))
    }
}
