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
