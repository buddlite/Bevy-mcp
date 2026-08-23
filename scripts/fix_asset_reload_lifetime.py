from pathlib import Path

path = Path("crates/bevy-mcp-host/src/systems.rs")
text = path.read_text()
old = "    asset_server.reload(path);"
new = "    asset_server.reload(path.to_owned());"
if text.count(old) != 1:
    raise SystemExit(f"expected one reload anchor, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))
