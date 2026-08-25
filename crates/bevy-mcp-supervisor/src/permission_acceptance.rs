use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::{
    CargoExecutor, CargoExecutorConfig, CargoInvocation, CargoOperationKind, CargoOperationSnapshot,
    CargoOperationState, ProcessManager, ProcessManagerConfig, ProcessOwnership, ProcessSnapshot,
    ProcessState, RebuildRestartCoordinator, SupervisorPermissions, SupervisorTransport,
};

const MANAGED_FIXTURE_SOURCE: &str = r#"
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
            host_version: "permission-fixture".into(),
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

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new(source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "bevy-mcp-permissions-{}-{}",
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
name = "permission_fixture"
version = "0.1.0"
edition = "2024"
build = "build.rs"

[dependencies]
bevy-mcp-core = {{ path = "{core}" }}
serde_json = "1"
"#
            ),
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), source).unwrap();
        fs::write(
            root.join("build.rs"),
            r#"fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    std::fs::write(root.join("cargo-spawned.marker"), b"spawned").unwrap();
}
"#,
        )
        .unwrap();
        Self { root }
    }

    fn spawn_marker(&self) -> PathBuf {
        self.root.join("cargo-spawned.marker")
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

async fn executor(project: &TempProject, permissions: SupervisorPermissions) -> CargoExecutor {
    let mut config = CargoExecutorConfig::new(&project.root);
    config.permissions = permissions;
    config.poll_interval = Duration::from_millis(10);
    config.check_timeout = Duration::from_secs(60);
    config.build_timeout = Duration::from_secs(60);
    config.test_timeout = Duration::from_secs(60);
    CargoExecutor::initialize(config).await
}

async fn assert_cargo_permission_denied(
    kind: CargoOperationKind,
    permissions: SupervisorPermissions,
) {
    let project = TempProject::new("fn main() {}\n");
    let executor = executor(&project, permissions).await;
    assert!(executor.available(), "fixture metadata should be available");
    assert!(
        !project.spawn_marker().exists(),
        "cargo metadata must not run build.rs"
    );

    let error = match kind {
        CargoOperationKind::Check => executor.start_check(invocation()),
        CargoOperationKind::Build => executor.start_build(invocation()),
        CargoOperationKind::Test => executor.start_test(invocation()),
    }
    .unwrap_err();

    assert_eq!(error.code, "SUPERVISOR_PERMISSION_DENIED");
    assert!(
        executor.status(None).unwrap().is_empty(),
        "a denied Cargo request must not create an operation record"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !project.spawn_marker().exists(),
        "a denied Cargo request must not spawn Cargo or execute build.rs"
    );
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

async fn manager() -> (SupervisorTransport, ProcessManager) {
    let token = format!("permission-secret-{}", Uuid::new_v4());
    let transport = SupervisorTransport::bind(
        format!("permission-bootstrap-{}", Uuid::new_v4()),
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

fn permissions_without(field: &str) -> SupervisorPermissions {
    let mut permissions = SupervisorPermissions::full();
    match field {
        "cargo_check" => permissions.cargo_check = false,
        "cargo_build" => permissions.cargo_build = false,
        "process_stop" => permissions.process_stop = false,
        "process_launch" => permissions.process_launch = false,
        other => panic!("unknown permission field {other}"),
    }
    permissions
}

fn assert_same_managed_process(before: &ProcessSnapshot, after: &ProcessSnapshot) {
    assert_eq!(after.state, ProcessState::Running);
    assert_eq!(after.ownership, ProcessOwnership::Managed);
    assert_eq!(after.pid, before.pid);
    assert_eq!(after.instance_id, before.instance_id);
    assert_eq!(after.connection_id, before.connection_id);
    assert_eq!(after.started_unix_ms, before.started_unix_ms);
}

#[test]
fn read_only_profile_disables_every_supervisor_mutation() {
    let permissions = SupervisorPermissions::read_only();
    assert!(!permissions.cargo_check);
    assert!(!permissions.cargo_build);
    assert!(!permissions.cargo_test);
    assert!(!permissions.process_launch);
    assert!(!permissions.process_stop);
    assert!(!permissions.process_restart);
}

#[tokio::test]
async fn denied_cargo_permissions_fail_before_operation_record_or_spawn() {
    let mut check = SupervisorPermissions::full();
    check.cargo_check = false;
    assert_cargo_permission_denied(CargoOperationKind::Check, check).await;

    let mut build = SupervisorPermissions::full();
    build.cargo_build = false;
    assert_cargo_permission_denied(CargoOperationKind::Build, build).await;

    let mut test = SupervisorPermissions::full();
    test.cargo_test = false;
    assert_cargo_permission_denied(CargoOperationKind::Test, test).await;
}

#[tokio::test]
async fn rebuild_restart_permission_denial_preserves_running_game_and_starts_no_cargo() {
    let project = TempProject::new(MANAGED_FIXTURE_SOURCE);
    let executor = executor(&project, SupervisorPermissions::full()).await;
    let (_transport, manager) = manager().await;
    let initial = build_and_launch(&executor, &manager).await;
    assert_eq!(initial.state, ProcessState::Running);
    assert_eq!(initial.ownership, ProcessOwnership::Managed);
    let cargo_operation_count = executor.status(None).unwrap().len();

    for field in [
        "cargo_check",
        "cargo_build",
        "process_stop",
        "process_launch",
    ] {
        let coordinator = RebuildRestartCoordinator::new(
            manager.clone(),
            executor.clone(),
            permissions_without(field),
        );
        let error = coordinator.start(invocation()).unwrap_err();
        assert_eq!(
            error.code, "SUPERVISOR_PERMISSION_DENIED",
            "field={field}"
        );
        assert_eq!(error.details[field], false, "field={field}");
        assert!(
            coordinator.status(None).unwrap().is_empty(),
            "denial must occur before a rebuild operation is recorded; field={field}"
        );
        assert_eq!(
            executor.status(None).unwrap().len(),
            cargo_operation_count,
            "denied rebuild must not start a Cargo operation; field={field}"
        );
        let after = manager.status().await;
        assert_same_managed_process(&initial, &after);
    }

    manager.stop().await.unwrap();
}
