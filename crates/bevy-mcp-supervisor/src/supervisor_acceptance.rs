use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;
use uuid::Uuid;

use crate::process_tools::{SupervisorCapabilityContext, merge_supervisor_capabilities};
use crate::{
    CargoExecutor, CargoExecutorConfig, CargoInvocation, CargoOperationSnapshot,
    CargoOperationState, DevelopmentState, DevelopmentStatus, ProcessManager, ProcessManagerConfig,
    ProcessOwnership, ProcessSnapshot, ProcessState, RebuildRestartCoordinator,
    RebuildRestartSnapshot, RebuildRestartState, SupervisorPermissions, SupervisorTransport,
};

const HEALTHY_SOURCE_V1: &str = r#"
use std::net::TcpStream;
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::wire::{
    DEFAULT_MAX_FRAME_SIZE, Hello, WireEnvelope, WireMessage, WireResponse, read_frame, write_frame,
};

fn main() {
    let address = std::env::var("BEVY_MCP_SUPERVISOR_ADDR").unwrap();
    let token = std::env::var("BEVY_MCP_SUPERVISOR_TOKEN").unwrap();
    let instance_id = std::env::var("BEVY_MCP_INSTANCE_ID").unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_nodelay(true).unwrap();
    write_frame(
        &mut stream,
        &WireEnvelope::new(WireMessage::Hello(Hello {
            token,
            instance_id: instance_id.clone(),
            host_version: "supervisor-fixture-v1".into(),
            bevy_version: None,
            pid: Some(std::process::id()),
        })),
        DEFAULT_MAX_FRAME_SIZE,
    )
    .unwrap();
    let accepted = read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE).unwrap();
    let connection_id = match accepted.message {
        WireMessage::HelloAccepted(accepted) => accepted.connection_id,
        other => panic!("unexpected handshake: {other:?}"),
    };
    loop {
        let envelope = match read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE) {
            Ok(envelope) => envelope,
            Err(_) => break,
        };
        match envelope.message {
            WireMessage::Command(command) => {
                if let McpCommand::HostProbe { probe_id } = command.command {
                    write_frame(
                        &mut stream,
                        &WireEnvelope::on_connection(
                            connection_id.clone(),
                            WireMessage::Response(WireResponse {
                                request_id: command.request_id,
                                result: McpResult::success(serde_json::json!({
                                    "probe_id": probe_id,
                                    "instance_id": instance_id,
                                    "frame": 1,
                                })),
                            }),
                        ),
                        DEFAULT_MAX_FRAME_SIZE,
                    )
                    .unwrap();
                }
            }
            WireMessage::TransportPing { nonce } => {
                write_frame(
                    &mut stream,
                    &WireEnvelope::on_connection(
                        connection_id.clone(),
                        WireMessage::TransportPong { nonce },
                    ),
                    DEFAULT_MAX_FRAME_SIZE,
                )
                .unwrap();
            }
            WireMessage::Shutdown(_) => break,
            _ => {}
        }
    }
}
"#;

const HEALTHY_SOURCE_V2: &str = r#"
use std::net::TcpStream;
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::wire::{
    DEFAULT_MAX_FRAME_SIZE, Hello, WireEnvelope, WireMessage, WireResponse, read_frame, write_frame,
};

fn main() {
    let address = std::env::var("BEVY_MCP_SUPERVISOR_ADDR").unwrap();
    let token = std::env::var("BEVY_MCP_SUPERVISOR_TOKEN").unwrap();
    let instance_id = std::env::var("BEVY_MCP_INSTANCE_ID").unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_nodelay(true).unwrap();
    write_frame(
        &mut stream,
        &WireEnvelope::new(WireMessage::Hello(Hello {
            token,
            instance_id: instance_id.clone(),
            host_version: "supervisor-fixture-v2".into(),
            bevy_version: None,
            pid: Some(std::process::id()),
        })),
        DEFAULT_MAX_FRAME_SIZE,
    )
    .unwrap();
    let accepted = read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE).unwrap();
    let connection_id = match accepted.message {
        WireMessage::HelloAccepted(accepted) => accepted.connection_id,
        other => panic!("unexpected handshake: {other:?}"),
    };
    loop {
        let envelope = match read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE) {
            Ok(envelope) => envelope,
            Err(_) => break,
        };
        match envelope.message {
            WireMessage::Command(command) => {
                if let McpCommand::HostProbe { probe_id } = command.command {
                    write_frame(
                        &mut stream,
                        &WireEnvelope::on_connection(
                            connection_id.clone(),
                            WireMessage::Response(WireResponse {
                                request_id: command.request_id,
                                result: McpResult::success(serde_json::json!({
                                    "probe_id": probe_id,
                                    "instance_id": instance_id,
                                    "frame": 2,
                                })),
                            }),
                        ),
                        DEFAULT_MAX_FRAME_SIZE,
                    )
                    .unwrap();
                }
            }
            WireMessage::TransportPing { nonce } => {
                write_frame(
                    &mut stream,
                    &WireEnvelope::on_connection(
                        connection_id.clone(),
                        WireMessage::TransportPong { nonce },
                    ),
                    DEFAULT_MAX_FRAME_SIZE,
                )
                .unwrap();
            }
            WireMessage::Shutdown(_) => break,
            _ => {}
        }
    }
}
"#;

const STARTUP_FAILURE_SOURCE: &str = r#"
fn main() {
    eprintln!("supervisor startup failure marker");
    std::process::exit(42);
}
"#;

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new(source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "bevy-mcp-supervisor-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        let core = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("bevy-mcp-core")
            .to_string_lossy()
            .replace('\\', "/");
        fs::write(
            root.join("Cargo.toml"),
            format!(
                r#"[package]
name = "supervisor_fixture"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy-mcp-core = {{ path = "{core}" }}
serde_json = "1"
"#
            ),
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), source).unwrap();
        Self { root }
    }

    fn write_source(&self, source: &str) {
        fs::write(self.root.join("src/main.rs"), source).unwrap();
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn invocation() -> CargoInvocation {
    CargoInvocation::new(None, None, None, None, None)
}

async fn executor(project: &TempProject) -> CargoExecutor {
    let mut config = CargoExecutorConfig::new(&project.root);
    config.poll_interval = Duration::from_millis(10);
    config.check_timeout = Duration::from_secs(60);
    config.build_timeout = Duration::from_secs(60);
    CargoExecutor::initialize(config).await
}

async fn wait_cargo(executor: &CargoExecutor, operation_id: &str) -> CargoOperationSnapshot {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let snapshot = executor
            .status(Some(operation_id))
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        if matches!(
            snapshot.state,
            CargoOperationState::Succeeded
                | CargoOperationState::Failed
                | CargoOperationState::Cancelled
                | CargoOperationState::TimedOut
        ) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "Cargo operation timed out in test"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_rebuild(
    coordinator: &RebuildRestartCoordinator,
    operation_id: &str,
) -> RebuildRestartSnapshot {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let snapshot = coordinator
            .status(Some(operation_id))
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        if matches!(
            snapshot.state,
            RebuildRestartState::Succeeded
                | RebuildRestartState::Failed
                | RebuildRestartState::Cancelled
        ) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "rebuild_restart operation timed out in test"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn manager() -> (SupervisorTransport, ProcessManager) {
    let token = format!("supervisor-secret-{}", Uuid::new_v4());
    let transport = SupervisorTransport::bind(
        format!("supervisor-bootstrap-{}", Uuid::new_v4()),
        token.clone(),
    )
    .await
    .unwrap();
    let manager = ProcessManager::new(
        transport.backend(),
        transport.address(),
        token,
        ProcessManagerConfig {
            launch: None,
            ready_timeout: Duration::from_secs(3),
            graceful_stop_timeout: Duration::from_secs(1),
            force_stop_timeout: Duration::from_secs(2),
            poll_interval: Duration::from_millis(10),
            log_capacity: 200,
        },
    );
    (transport, manager)
}

async fn build_and_launch(executor: &CargoExecutor, manager: &ProcessManager) -> ProcessSnapshot {
    let build = executor.start_build(invocation()).unwrap();
    let build = wait_cargo(executor, &build.operation_id).await;
    assert_eq!(build.state, CargoOperationState::Succeeded);
    let executable = build
        .result
        .as_ref()
        .and_then(|result| result.executable.as_ref())
        .expect("fixture build should return executable");
    manager.launch_artifact(executable).await.unwrap()
}

#[tokio::test]
async fn rebuild_restart_rotates_identity_and_launches_cargo_artifact() {
    let project = TempProject::new(HEALTHY_SOURCE_V1);
    let executor = executor(&project).await;
    let (_transport, manager) = manager().await;
    let initial = build_and_launch(&executor, &manager).await;
    let old_instance = initial.instance_id.clone().unwrap();
    let old_connection = initial.connection_id.clone().unwrap();

    project.write_source(HEALTHY_SOURCE_V2);
    let coordinator = RebuildRestartCoordinator::new(
        manager.clone(),
        executor.clone(),
        SupervisorPermissions::full(),
    );
    let operation = coordinator.start(invocation()).unwrap();
    assert!(
        operation
            .operation_id
            .starts_with("supervisor:rebuild_restart:")
    );
    let result = wait_rebuild(&coordinator, &operation.operation_id).await;

    assert_eq!(result.state, RebuildRestartState::Succeeded);
    assert_eq!(
        result.evidence.check.as_ref().unwrap().state,
        CargoOperationState::Succeeded
    );
    assert_eq!(
        result.evidence.build.as_ref().unwrap().state,
        CargoOperationState::Succeeded
    );
    let launched = result.evidence.launched_process.as_ref().unwrap();
    assert_eq!(launched.state, ProcessState::Running);
    assert_eq!(launched.host, "ready");
    assert_ne!(launched.instance_id.as_deref(), Some(old_instance.as_str()));
    assert_ne!(
        launched.connection_id.as_deref(),
        Some(old_connection.as_str())
    );
    assert!(
        result
            .evidence
            .executable
            .as_ref()
            .is_some_and(|path| PathBuf::from(path).exists())
    );
    manager.stop().await.unwrap();
}

#[tokio::test]
async fn failed_preflight_check_leaves_old_game_running() {
    let project = TempProject::new(HEALTHY_SOURCE_V1);
    let executor = executor(&project).await;
    let (_transport, manager) = manager().await;
    let initial = build_and_launch(&executor, &manager).await;
    let initial_instance = initial.instance_id.clone();
    let initial_connection = initial.connection_id.clone();

    project.write_source("fn main() { let _ = definitely_missing_symbol; }\n");
    let coordinator = RebuildRestartCoordinator::new(
        manager.clone(),
        executor.clone(),
        SupervisorPermissions::full(),
    );
    let operation = coordinator.start(invocation()).unwrap();
    let result = wait_rebuild(&coordinator, &operation.operation_id).await;

    assert_eq!(result.state, RebuildRestartState::Failed);
    let failure = result.failure.as_ref().unwrap();
    assert_eq!(failure.stage, "check");
    assert_eq!(failure.code, "BUILD_FAILED");
    let current = manager.status().await;
    assert_eq!(current.state, ProcessState::Running);
    assert_eq!(current.ownership, ProcessOwnership::Managed);
    assert_eq!(current.instance_id, initial_instance);
    assert_eq!(current.connection_id, initial_connection);

    let development = DevelopmentStatus::collect(
        &manager,
        &executor,
        &coordinator,
        SupervisorPermissions::full(),
        50,
    )
    .await;
    assert_eq!(development.state, DevelopmentState::CompileFailed);
    let diagnostic_failure = development.last_failure.as_ref().unwrap();
    assert_eq!(diagnostic_failure.source, "rebuild_restart");
    assert_eq!(diagnostic_failure.stage.as_deref(), Some("check"));
    assert!(!diagnostic_failure.diagnostics.is_empty());
    assert_eq!(development.recovery.action, "fix_compile_errors");
    assert_eq!(
        development.recovery.tool.as_deref(),
        Some("rebuild_restart")
    );

    manager.stop().await.unwrap();
}

#[tokio::test]
async fn replacement_startup_failure_returns_exit_and_stderr_evidence() {
    let project = TempProject::new(HEALTHY_SOURCE_V1);
    let executor = executor(&project).await;
    let (_transport, manager) = manager().await;
    build_and_launch(&executor, &manager).await;

    project.write_source(STARTUP_FAILURE_SOURCE);
    let coordinator = RebuildRestartCoordinator::new(
        manager.clone(),
        executor.clone(),
        SupervisorPermissions::full(),
    );
    let operation = coordinator.start(invocation()).unwrap();
    let result = wait_rebuild(&coordinator, &operation.operation_id).await;

    assert_eq!(result.state, RebuildRestartState::Failed);
    let failure = result.failure.as_ref().unwrap();
    assert_eq!(failure.stage, "launch");
    assert_eq!(failure.code, "PROCESS_EXITED_DURING_STARTUP");
    assert_eq!(failure.details["exit_code"], 42);
    assert!(
        failure
            .details
            .to_string()
            .contains("supervisor startup failure marker")
    );

    let development = DevelopmentStatus::collect(
        &manager,
        &executor,
        &coordinator,
        SupervisorPermissions::full(),
        50,
    )
    .await;
    assert_eq!(development.state, DevelopmentState::StartupFailed);
    let startup_failure = development.last_failure.as_ref().unwrap();
    assert_eq!(startup_failure.stage.as_deref(), Some("launch"));
    assert!(
        startup_failure
            .stderr_tail
            .iter()
            .any(|entry| entry.text.contains("supervisor startup failure marker"))
    );
    assert_eq!(
        development.recovery.tool.as_deref(),
        Some("process_evidence")
    );
}

#[test]
fn merged_capabilities_replace_embedded_build_and_lifecycle_contract() {
    let host = json!({
        "schema_version": 2,
        "permissions": { "build": false },
        "runtime": {
            "launch": { "implemented": false },
            "stop": { "implemented": false },
            "restart": { "implemented": false }
        },
        "build": {
            "check": { "implemented": false },
            "build": { "implemented": false },
            "test": { "implemented": false }
        }
    });
    let process = ProcessSnapshot {
        state: ProcessState::Stopped,
        ownership: ProcessOwnership::None,
        pid: None,
        instance_id: Some("run-supervisor".into()),
        connection_id: None,
        transport: "disconnected".into(),
        host: "waiting".into(),
        exit_code: None,
        executable: None,
        started_unix_ms: None,
        exited_unix_ms: None,
        last_error: None,
    };
    let merged = merge_supervisor_capabilities(
        host,
        SupervisorCapabilityContext {
            connected: false,
            ready: false,
            instance_id: Some("run-supervisor".into()),
            connection_id: None,
            process: &process,
            configured_launch_target: false,
            cargo_available: true,
            permissions: SupervisorPermissions::full(),
            cargo_error: None,
            host_error: None,
        },
    );

    assert_eq!(merged["mode"], "supervised");
    assert_eq!(merged["build"]["check"]["operational"], true);
    assert_eq!(merged["build"]["build"]["operational"], true);
    assert_eq!(merged["build"]["test"]["operational"], true);
    assert_eq!(merged["runtime"]["launch"]["operational"], false);
    assert_eq!(merged["runtime"]["rebuild_restart"]["operational"], true);
    assert_eq!(merged["supervisor"]["cargo"]["available"], true);
}
