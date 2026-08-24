use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::{
    CargoExecutor, CargoExecutorConfig, CargoInvocation, CargoOperationSnapshot,
    CargoOperationState,
};

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new(cargo_toml: &str, files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "bevy-mcp-stage3-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.toml"), cargo_toml).unwrap();
        for (path, contents) in files {
            let path = root.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, contents).unwrap();
        }
        Self { root }
    }

    fn simple(main_rs: &str) -> Self {
        Self::new(
            r#"[package]
name = "stage3_fixture"
version = "0.1.0"
edition = "2024"

[features]
known = []
"#,
            &[("src/main.rs", main_rs)],
        )
    }

    fn with_build_script(build_rs: &str) -> Self {
        Self::new(
            r#"[package]
name = "stage3_slow_fixture"
version = "0.1.0"
edition = "2024"
build = "build.rs"
"#,
            &[("src/main.rs", "fn main() {}\n"), ("build.rs", build_rs)],
        )
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

fn executor_config(project: &TempProject) -> CargoExecutorConfig {
    let mut config = CargoExecutorConfig::new(&project.root);
    config.poll_interval = Duration::from_millis(10);
    config.check_timeout = Duration::from_secs(30);
    config.build_timeout = Duration::from_secs(30);
    config.test_timeout = Duration::from_secs(30);
    config
}

async fn wait_terminal(
    executor: &CargoExecutor,
    operation_id: &str,
    timeout: Duration,
) -> CargoOperationSnapshot {
    let deadline = Instant::now() + timeout;
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
            "operation {operation_id} did not reach a terminal state"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_file(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn broken_project_returns_structured_compiler_diagnostic() {
    let project = TempProject::simple("fn main() { let _ = missing_symbol; }\n");
    let executor = CargoExecutor::initialize(executor_config(&project)).await;
    assert!(executor.available());

    let operation = executor.start_check(invocation()).unwrap();
    let snapshot = wait_terminal(&executor, &operation.operation_id, Duration::from_secs(30)).await;

    assert_eq!(snapshot.state, CargoOperationState::Failed);
    assert_eq!(snapshot.failure.as_ref().unwrap().code, "BUILD_FAILED");
    let result = snapshot.result.as_ref().unwrap();
    assert!(!result.success);
    assert!(result.error_count >= 1);
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.level == "error")
        .expect("expected at least one compiler error diagnostic");
    assert!(!diagnostic.message.is_empty());
    assert!(diagnostic.spans.iter().any(|span| {
        span.is_primary
            && span.file_name.replace('\\', "/").ends_with("src/main.rs")
            && span.line_start >= 1
            && span.column_start >= 1
    }));
}

#[tokio::test]
async fn successful_build_returns_cargo_reported_executable() {
    let project = TempProject::simple("fn main() { println!(\"stage3\"); }\n");
    let executor = CargoExecutor::initialize(executor_config(&project)).await;

    let operation = executor.start_build(invocation()).unwrap();
    let snapshot = wait_terminal(&executor, &operation.operation_id, Duration::from_secs(30)).await;

    assert_eq!(snapshot.state, CargoOperationState::Succeeded);
    let result = snapshot.result.as_ref().unwrap();
    assert!(result.success);
    let executable = result
        .executable
        .as_ref()
        .expect("Cargo compiler-artifact should report an executable");
    assert!(Path::new(executable).exists(), "{executable} should exist");
}

#[tokio::test]
async fn invalid_feature_and_ambiguous_target_are_rejected_before_operation_creation() {
    let project = TempProject::simple("fn main() {}\n");
    let executor = CargoExecutor::initialize(executor_config(&project)).await;
    let error = executor
        .start_check(CargoInvocation::new(
            None,
            None,
            None,
            Some(vec!["not-a-feature".to_string()]),
            None,
        ))
        .unwrap_err();
    assert_eq!(error.code, "FEATURE_UNKNOWN");
    assert!(executor.status(None).unwrap().is_empty());

    let ambiguous = TempProject::new(
        r#"[package]
name = "stage3_ambiguous_fixture"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "client"
path = "src/client.rs"

[[bin]]
name = "server"
path = "src/server.rs"
"#,
        &[
            ("src/client.rs", "fn main() {}\n"),
            ("src/server.rs", "fn main() {}\n"),
        ],
    );
    let executor = CargoExecutor::initialize(executor_config(&ambiguous)).await;
    let error = executor.start_check(invocation()).unwrap_err();
    assert_eq!(error.code, "TARGET_AMBIGUOUS");
    assert!(executor.status(None).unwrap().is_empty());
}

#[tokio::test]
async fn operation_start_is_prompt_and_second_operation_is_deterministically_rejected() {
    let project = TempProject::with_build_script(
        "fn main() { std::thread::sleep(std::time::Duration::from_secs(30)); }\n",
    );
    let executor = CargoExecutor::initialize(executor_config(&project)).await;

    let started = Instant::now();
    let first = executor.start_build(invocation()).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "start_build should return an operation ID promptly"
    );
    assert!(first.operation_id.starts_with("supervisor:build:"));

    let error = executor.start_check(invocation()).unwrap_err();
    assert_eq!(error.code, "CARGO_OPERATION_IN_PROGRESS");

    executor.cancel(&first.operation_id).await.unwrap();
    let snapshot = wait_terminal(&executor, &first.operation_id, Duration::from_secs(10)).await;
    assert_eq!(snapshot.state, CargoOperationState::Cancelled);
    assert_eq!(snapshot.failure.as_ref().unwrap().code, "BUILD_CANCELLED");
}

#[cfg(target_os = "linux")]
fn descendant_build_script() -> &'static str {
    r#"use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn main() {
    let root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let child = Command::new("sleep").arg("60").spawn().unwrap();
    fs::write(Path::new(&root).join("descendant.pid"), child.id().to_string()).unwrap();
    std::mem::forget(child);
    thread::sleep(Duration::from_secs(60));
}
"#
}

#[cfg(target_os = "linux")]
async fn assert_process_gone(pid: u32) {
    let proc_path = PathBuf::from(format!("/proc/{pid}"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while proc_path.exists() {
        assert!(
            Instant::now() < deadline,
            "descendant process {pid} survived Cargo process-tree termination"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cancellation_terminates_cargo_descendants() {
    let project = TempProject::with_build_script(descendant_build_script());
    let executor = CargoExecutor::initialize(executor_config(&project)).await;
    let operation = executor.start_build(invocation()).unwrap();

    let pid_path = project.root.join("descendant.pid");
    wait_for_file(&pid_path, Duration::from_secs(15)).await;
    let pid: u32 = fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    executor.cancel(&operation.operation_id).await.unwrap();
    let snapshot = wait_terminal(&executor, &operation.operation_id, Duration::from_secs(10)).await;
    assert_eq!(snapshot.state, CargoOperationState::Cancelled);
    assert_process_gone(pid).await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn timeout_terminates_cargo_descendants() {
    let project = TempProject::with_build_script(descendant_build_script());
    let mut config = executor_config(&project);
    config.build_timeout = Duration::from_secs(5);
    let executor = CargoExecutor::initialize(config).await;
    let operation = executor.start_build(invocation()).unwrap();

    let pid_path = project.root.join("descendant.pid");
    wait_for_file(&pid_path, Duration::from_secs(15)).await;
    let pid: u32 = fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    let snapshot = wait_terminal(&executor, &operation.operation_id, Duration::from_secs(15)).await;
    assert_eq!(snapshot.state, CargoOperationState::TimedOut);
    assert_eq!(snapshot.failure.as_ref().unwrap().code, "BUILD_TIMEOUT");
    assert_process_gone(pid).await;
}
