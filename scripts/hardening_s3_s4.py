from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one anchor, found {count}")
    file.write_text(text.replace(old, new, 1))


replace_once(
    "crates/bevy-mcp-host/src/checkpoint.rs",
    '''    pub fn start_replay(
        &mut self,
        recording_id: String,
        checkpoint_id: Option<String>,
        frame: u64,
    ) -> Result<String, String> {
        if !self.recordings.contains_key(&recording_id) {
            return Err(format!("Recording '{recording_id}' not found"));
        }
        if self
            .replays
            .values()
            .any(|r| r.status == ReplayStatus::Running)
        {
            return Err("A replay is already running".into());
        }
        let id = self.alloc("replay");
''',
    '''    pub fn validate_replay_start(&self, recording_id: &str) -> Result<(), String> {
        if !self.recordings.contains_key(recording_id) {
            return Err(format!("Recording '{recording_id}' not found"));
        }
        if self
            .replays
            .values()
            .any(|r| r.status == ReplayStatus::Running)
        {
            return Err("A replay is already running".into());
        }
        Ok(())
    }

    pub fn start_replay(
        &mut self,
        recording_id: String,
        checkpoint_id: Option<String>,
        frame: u64,
    ) -> Result<String, String> {
        self.validate_replay_start(&recording_id)?;
        let id = self.alloc("replay");
''',
)

replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    '''        DebugRequest::ReplayStart {
            recording_id,
            checkpoint_id,
        } => {
            if let Some(checkpoint_id) = checkpoint_id.as_ref() {
''',
    '''        DebugRequest::ReplayStart {
            recording_id,
            checkpoint_id,
        } => {
            if let Err(error) = world
                .resource::<McpRecorder>()
                .validate_replay_start(&recording_id)
            {
                push_error(world, request_id, "REPLAY_START_FAILED", error);
                return;
            }

            if let Some(checkpoint_id) = checkpoint_id.as_ref() {
''',
)

test_path = Path("crates/bevy-mcp-host/tests/intelligence.rs")
text = test_path.read_text()
replace = '''    let replay_id = recorder
        .start_replay(recording.id.clone(), Some("checkpoint-1".into()), 500)
        .unwrap();
    let replay = recorder.replays.get(&replay_id).unwrap();
'''
replacement = '''    assert!(recorder.validate_replay_start("missing-recording").is_err());
    assert!(recorder.validate_replay_start(&recording.id).is_ok());

    let replay_id = recorder
        .start_replay(recording.id.clone(), Some("checkpoint-1".into()), 500)
        .unwrap();
    assert!(recorder.validate_replay_start(&recording.id).is_err());
    let replay = recorder.replays.get(&replay_id).unwrap();
'''
if text.count(replace) != 1:
    raise SystemExit("tests: replay anchor mismatch")
test_path.write_text(text.replace(replace, replacement, 1))
