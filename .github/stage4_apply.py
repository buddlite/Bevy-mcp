from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    if old not in text:
        raise SystemExit(f"expected patch anchor missing in {path}: {old[:80]!r}")
    path.write_text(text.replace(old, new, 1))


# ProcessManager: allow the Stage 4 coordinator to launch the exact Cargo-reported
# executable while preserving configured args/cwd/env as a launch template.
path = Path("crates/bevy-mcp-supervisor/src/process_manager.rs")
old = '''    pub async fn launch(&self) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        self.launch_inner().await
    }

    async fn launch_inner(&self) -> Result<ProcessSnapshot, ProcessError> {
        let launch = self.inner.config.launch.clone().ok_or_else(|| {
            ProcessError::new(
                "PROCESS_TARGET_NOT_CONFIGURED",
                "No managed game executable was configured when the supervisor started",
            )
        })?;

        if self.inner.child.lock().await.is_some() {
'''
new = '''    pub async fn launch(&self) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        self.launch_inner().await
    }

    /// Launch the exact executable path reported by Cargo while preserving any
    /// configured game args/current-dir/environment as a launch template.
    /// When no explicit launch target was configured, the artifact is launched
    /// directly with the supervisor's current working directory.
    pub async fn launch_artifact(
        &self,
        executable: impl Into<PathBuf>,
    ) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        let executable = executable.into();
        let mut launch = self
            .inner
            .config
            .launch
            .clone()
            .unwrap_or_else(|| LaunchSpec::new(executable.clone()));
        launch.executable = executable;
        self.launch_spec_inner(launch).await
    }

    pub fn has_configured_launch_target(&self) -> bool {
        self.inner.config.launch.is_some()
    }

    async fn launch_inner(&self) -> Result<ProcessSnapshot, ProcessError> {
        let launch = self.inner.config.launch.clone().ok_or_else(|| {
            ProcessError::new(
                "PROCESS_TARGET_NOT_CONFIGURED",
                "No managed game executable was configured when the supervisor started",
            )
        })?;
        self.launch_spec_inner(launch).await
    }

    async fn launch_spec_inner(&self, launch: LaunchSpec) -> Result<ProcessSnapshot, ProcessError> {
        if self.inner.child.lock().await.is_some() {
'''
replace_once(path, old, new)

# Rebuild failure evidence needs one async process-status read; never block_on a
# Tokio runtime from inside an async task.
path = Path("crates/bevy-mcp-supervisor/src/rebuild_restart.rs")
replace_once(
    path,
    '                    self.finish_process_failure(&operation_id, "stop", error);',
    '                    self.finish_process_failure(&operation_id, "stop", error).await;',
)
replace_once(
    path,
    '                self.finish_process_failure(&operation_id, "launch", error);',
    '                self.finish_process_failure(&operation_id, "launch", error).await;',
)
old = '''    fn finish_process_failure(&self, operation_id: &str, stage: &str, error: ProcessError) {
        let mut details = error.details.clone();
        if !details.is_object() {
            details = json!({ "process_error_details": details });
        }
        if let Some(object) = details.as_object_mut() {
            object.insert(
                "process".to_string(),
                serde_json::to_value(
                    tokio::runtime::Handle::current().block_on(self.inner.manager.status()),
                )
                .unwrap_or(Value::Null),
            );
            object.insert(
                "stderr_tail".to_string(),
                serde_json::to_value(self.inner.manager.logs(Some("stderr"), 50).unwrap_or_default())
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "stdout_tail".to_string(),
                serde_json::to_value(self.inner.manager.logs(Some("stdout"), 50).unwrap_or_default())
                    .unwrap_or(Value::Null),
            );
        }
        self.finish_failure(operation_id, error.code, &error.message, stage, details);
    }
'''
new = '''    async fn finish_process_failure(
        &self,
        operation_id: &str,
        stage: &str,
        error: ProcessError,
    ) {
        let process = self.inner.manager.status().await;
        let mut details = error.details.clone();
        if !details.is_object() {
            details = json!({ "process_error_details": details });
        }
        if let Some(object) = details.as_object_mut() {
            object.insert(
                "process".to_string(),
                serde_json::to_value(process).unwrap_or(Value::Null),
            );
            object.insert(
                "stderr_tail".to_string(),
                serde_json::to_value(self.inner.manager.logs(Some("stderr"), 50).unwrap_or_default())
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "stdout_tail".to_string(),
                serde_json::to_value(self.inner.manager.logs(Some("stdout"), 50).unwrap_or_default())
                    .unwrap_or(Value::Null),
            );
        }
        self.finish_failure(operation_id, error.code, &error.message, stage, details);
    }
'''
replace_once(path, old, new)

# Changelog reconciliation.
path = Path("CHANGELOG.md")
replace_once(
    path,
    '- Supervisor Cargo execution for `build_check`, `build`, and `test`, with `cargo metadata` target discovery, typed package/bin/profile/features/test-filter parameters, structured compiler diagnostics and executable artifacts, bounded output, asynchronous `supervisor:*` operation IDs, cancellation/timeouts, one-operation-at-a-time locking, and supervisor-local permissions.\n',
    '- Supervisor Cargo execution for `build_check`, `build`, and `test`, with `cargo metadata` target discovery, typed package/bin/profile/features/test-filter parameters, structured compiler diagnostics and executable artifacts, bounded output, asynchronous `supervisor:*` operation IDs, cancellation/timeouts, one-operation-at-a-time locking, and supervisor-local permissions.\n- Stage 4 supervised development-cycle tooling: asynchronous `rebuild_restart`, conservative check-before-stop sequencing, Cargo-artifact launch, new instance/connection validation, merged host/supervisor capabilities, and bounded startup/crash evidence.\n',
)
replace_once(
    path,
    '- In supervised mode, build/test and OS process lifecycle authority live in the persistent supervisor rather than the Bevy host; embedded mode retains the existing game-local permission boundary and externally owned lifecycle.\n',
    '- In supervised mode, build/test and OS process lifecycle authority live in the persistent supervisor rather than the Bevy host; embedded mode retains the existing game-local permission boundary and externally owned lifecycle.\n- The supervised `capabilities` response now merges the live Bevy-host contract with supervisor Cargo, process, and `rebuild_restart` availability instead of exposing the embedded build/lifecycle contract unchanged.\n',
)
replace_once(
    path,
    '- Embedded `build_check`, `build`, and `test` tools return `BUILD_NOT_AVAILABLE`; supervisor mode provides the Stage 3 Cargo executor instead.\n',
    '- Embedded `build_check`, `build`, and `test` tools return `BUILD_NOT_AVAILABLE`; supervisor mode provides the trusted Cargo executor instead.\n',
)
replace_once(
    path,
    '- The Stage 4 `rebuild_restart` composite autonomous development cycle and full supervised-mode onboarding/documentation are not implemented yet.\n',
    '- Supervisor Cargo operations execute project build scripts/proc macros as trusted local development code; keep supervisor build permissions disabled for untrusted projects.\n',
)

# Front-page documentation: distinguish embedded and supervised modes and route
# full onboarding to the dedicated guide.
path = Path("README.md")
replace_once(
    path,
    'bevy-mcp embeds an MCP host directly in your Bevy application and exposes the live world to an MCP-compatible coding agent. The client still talks MCP over stdio, but ECS reads, deferred mutations, debugging state, input injection, and runtime control are handled inside the game process instead of through an external engine bridge.\n',
    'bevy-mcp supports two execution modes over the same MCP tool model. **Embedded mode** keeps the MCP server and Bevy host in one process for direct runtime inspection/control. **Supervised mode** keeps a persistent `bevy-mcp` control-plane process alive while game binaries rebuild and restart underneath it, so a coding agent can survive compile errors, crashes, and process replacement without losing its MCP session.\n',
)
replace_once(
    path,
    'Call `capabilities` to get the live host contract. It reports whether a feature is implemented, currently available in this app/runtime, and allowed by the active permission level. Agents should prefer this over assuming that every registered MCP tool is usable in every game configuration.\n',
    'Call `capabilities` to get the live contract. In embedded mode it reports the Bevy-host surface. In supervised mode the persistent supervisor merges that host contract with Cargo/build permissions, managed-process lifecycle state, and `rebuild_restart` availability. Agents should prefer this over assuming that every registered MCP tool is usable in every game configuration.\n',
)
replace_once(
    path,
    '- **Embedded build tools are disabled.** `build_check`, `build`, and `test` return `BUILD_NOT_AVAILABLE`; run Cargo commands from a trusted development shell.\n',
    '- **Build tools are mode-dependent.** Embedded `build_check`, `build`, and `test` remain unavailable; supervised mode owns trusted Cargo execution and the composite `rebuild_restart` development cycle.\n',
)
marker = '---\n\n## How it works\n'
insert = '''---

## Supervised mode for autonomous development

For coding agents that need to edit Rust, compile, relaunch, and continue interacting with the new game process, use the persistent supervisor rather than making the game binary itself the MCP stdio server.

The intended loop is:

```text
edit source -> rebuild_restart -> cargo check while old game stays live
                              -> stop old game only after check passes
                              -> cargo build -> launch Cargo-reported artifact
                              -> authenticated reconnect -> host probe -> ready
                              -> inspect/interact/assert/debug
```

`rebuild_restart` is asynchronous and returns a `supervisor:rebuild_restart:*` operation ID. Poll it with `operation_status`; use `process_evidence` for bounded stdout/stderr plus process state when startup or runtime failures need diagnosis. A failed preflight check leaves the old managed game untouched. A build failure after the stop phase deliberately leaves the game stopped rather than relaunching stale code.

See **[Supervised mode and autonomous rebuild/restart](docs/supervised-mode.md)** for game instrumentation, MCP client configuration, zero-config target discovery, lifecycle permissions, failure semantics, and troubleshooting.

---

## How it works
'''
replace_once(path, marker, insert)
replace_once(
    path,
    'The workspace is split into three crates:\n',
    'The workspace is split into four crates:\n',
)
replace_once(
    path,
    '- **`bevy-mcp-host`** — the Bevy plugin and runtime integration layer.\n',
    '- **`bevy-mcp-host`** — the Bevy plugin and runtime integration layer.\n- **`bevy-mcp-supervisor`** — the persistent control plane for authenticated game reconnection, Cargo execution, process ownership, evidence capture, and `rebuild_restart`.\n',
)
