from pathlib import Path

path = Path(__file__).resolve().parent / "bootstrap_supervisor_stage2.py"
text = path.read_text()

old = '''text = replace_once(
    text,
    "    expected_instance_id: String,\\n",
    "    expected_instance_id: Mutex<String>,\\n",
    "mutable expected instance",
)'''
new = '''text = replace_once(
    text,
    "struct Inner {\\n    expected_instance_id: String,\\n",
    "struct Inner {\\n    expected_instance_id: Mutex<String>,\\n",
    "mutable expected instance",
)'''
if old not in text:
    raise SystemExit("expected-instance bootstrap edit anchor not found")
text = text.replace(old, new, 1)

old = '''text = text.replace("backend.inner.expected_instance_id", "expected_instance_id")
# The broad replacement also touched methods above; repair the intended Mutex accesses.
text = text.replace("self.inner.expected_instance_id.lock()", "self.inner.expected_instance_id.lock()")
text = text.replace("*expected_instance_id.lock().unwrap()", "*self.inner.expected_instance_id.lock().unwrap()")
# Repair helper if broad replacement changed it.
text = text.replace("self.expected_instance_id.lock().unwrap().clone()", "self.inner.expected_instance_id.lock().unwrap().clone()")
# HelloAccepted should clone the handshake snapshot.
text = text.replace("instance_id: expected_instance_id.clone().clone(),", "instance_id: expected_instance_id.clone(),")
text = text.replace("instance_id: expected_instance_id.clone(),", "instance_id: expected_instance_id.clone(),")'''
new = '''text = replace_once(
    text,
    "hello.instance_id != backend.inner.expected_instance_id",
    "hello.instance_id != expected_instance_id",
    "handshake instance comparison",
)
text = replace_once(
    text,
    "backend.inner.expected_instance_id, hello.instance_id",
    "expected_instance_id, hello.instance_id",
    "handshake instance diagnostic",
)
text = replace_once(
    text,
    "instance_id: backend.inner.expected_instance_id.clone(),",
    "instance_id: expected_instance_id.clone(),",
    "handshake accepted instance",
)'''
if old not in text:
    raise SystemExit("broad expected-instance replacement block not found")
text = text.replace(old, new, 1)

# Preserve whether a managed target was configured before moving the Option into LaunchSpec.
old = '''    let cli = Cli::parse();
    let instance_id = generate_instance_id();'''
new = '''    let cli = Cli::parse();
    let has_managed_target = cli.game_executable.is_some();
    let instance_id = generate_instance_id();'''
if old not in text:
    raise SystemExit("CLI parse anchor not found")
text = text.replace(old, new, 1)

old = '''    if manager.status().await.executable.is_none() {'''
new = '''    if !has_managed_target {'''
if old not in text:
    raise SystemExit("managed-target display anchor not found")
text = text.replace(old, new, 1)

path.write_text(text)
print("Stage 2 bootstrap transformations prepared")
