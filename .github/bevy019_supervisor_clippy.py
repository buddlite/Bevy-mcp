from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: str, old: str, new: str, label: str) -> None:
    full = ROOT / path
    text = full.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 exact match, found {count}")
    full.write_text(text.replace(old, new, 1))


replace_once(
    "crates/bevy-mcp-supervisor/src/backend.rs",
    '''        if let Some(required) = required_connection_id {
            if active.connection_id != required {
                return Err(GameCallError::new(
                    "CONNECTION_REPLACED",
                    "The game connection generation changed before the command was sent",
                ));
            }
        }''',
    '''        if let Some(required) = required_connection_id
            && active.connection_id != required
        {
            return Err(GameCallError::new(
                "CONNECTION_REPLACED",
                "The game connection generation changed before the command was sent",
            ));
        }''',
    "backend connection guard",
)

replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''        if let Some(managed) = child.as_mut() {
            if managed.operation_id == operation_id {
                managed.child.start_kill().map_err(|error| {
                    CargoError::new(
                        "BUILD_CANCEL_FAILED",
                        format!("Failed to terminate Cargo process tree: {error}"),
                    )
                })?;
            }
        }''',
    '''        if let Some(managed) = child.as_mut()
            && managed.operation_id == operation_id
        {
            managed.child.start_kill().map_err(|error| {
                CargoError::new(
                    "BUILD_CANCEL_FAILED",
                    format!("Failed to terminate Cargo process tree: {error}"),
                )
            })?;
        }''',
    "cargo cancel child guard",
)

replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''        if kind == CargoOperationKind::Test {
            if let Some(filter) = invocation.filter.as_ref() {
                if filter.starts_with('-') {
                    return Err(CargoError::new(
                        "INVALID_TEST_FILTER",
                        "test filter must be a test-name filter, not a test-harness flag",
                    ));
                }
                args.push("--".to_string());
                args.push(filter.clone());
            }
        }''',
    '''        if kind == CargoOperationKind::Test
            && let Some(filter) = invocation.filter.as_ref()
        {
            if filter.starts_with('-') {
                return Err(CargoError::new(
                    "INVALID_TEST_FILTER",
                    "test filter must be a test-name filter, not a test-harness flag",
                ));
            }
            args.push("--".to_string());
            args.push(filter.clone());
        }''',
    "cargo test filter guard",
)

replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''        if let Some(managed) = child.as_mut() {
            if managed.operation_id == operation_id {
                managed
                    .child
                    .start_kill()
                    .map_err(|error| format!("Failed to kill Cargo process tree: {error}"))?;
            }
        }''',
    '''        if let Some(managed) = child.as_mut()
            && managed.operation_id == operation_id
        {
            managed
                .child
                .start_kill()
                .map_err(|error| format!("Failed to kill Cargo process tree: {error}"))?;
        }''',
    "cargo kill child guard",
)

replace_once(
    "crates/bevy-mcp-supervisor/src/process_manager.rs",
    '''        if let Some(stream) = stream {
            if stream != "stdout" && stream != "stderr" {
                return Err(ProcessError::new(
                    "INVALID_PROCESS_LOG_STREAM",
                    "stream must be 'stdout', 'stderr', or omitted",
                ));
            }
        }''',
    '''        if let Some(stream) = stream
            && stream != "stdout"
            && stream != "stderr"
        {
            return Err(ProcessError::new(
                "INVALID_PROCESS_LOG_STREAM",
                "stream must be 'stdout', 'stderr', or omitted",
            ));
        }''',
    "process log stream guard",
)

replace_once(
    "crates/bevy-mcp-supervisor/src/process_manager.rs",
    '''                    if let Some(value) = entry.text.strip_prefix("DESCENDANT_PORT=") {
                        if let Ok(port) = value.parse() {
                            return port;
                        }
                    }''',
    '''                    if let Some(value) = entry.text.strip_prefix("DESCENDANT_PORT=")
                        && let Ok(port) = value.parse()
                    {
                        return port;
                    }''',
    "descendant port parser",
)

replace_once(
    "crates/bevy-mcp-supervisor/src/process_tools.rs",
    '''        if matches!(
            request.name.as_ref(),
            "operation_status" | "operation_cancel"
        ) {
            if let Some(operation_id) = operation_id_from_request(&request) {
                if !operation_id.starts_with("supervisor:") {
                    return self.base.call_tool(request, context).await;
                }
            }
        }''',
    '''        if matches!(
            request.name.as_ref(),
            "operation_status" | "operation_cancel"
        )
            && let Some(operation_id) = operation_id_from_request(&request)
            && !operation_id.starts_with("supervisor:")
        {
            return self.base.call_tool(request, context).await;
        }''',
    "operation routing guard",
)

print("Applied Rust 1.98 supervisor Clippy cleanup")
