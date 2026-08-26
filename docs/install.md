# Install bevy-mcp

The easiest way to install the persistent `bevy-mcp` supervisor is from a GitHub Release.

## Windows

1. Open the repository's **Releases** page.
2. Download `bevy-mcp-windows-x86_64.zip` from the release you want.
3. Extract the archive somewhere permanent, for example `C:\Tools\bevy-mcp`.
4. Point your MCP client at `bevy-mcp.exe`.

Optional integrity check in PowerShell:

```powershell
Get-FileHash .\bevy-mcp-windows-x86_64.zip -Algorithm SHA256
```

Compare the result with `bevy-mcp-windows-x86_64.zip.sha256` from the same release.

## Linux

1. Open the repository's **Releases** page.
2. Download `bevy-mcp-linux-x86_64.tar.gz` from the release you want.
3. Extract it:

```bash
tar -xzf bevy-mcp-linux-x86_64.tar.gz
```

4. Run or configure your MCP client to use the extracted `bevy-mcp` binary.

Optional integrity check:

```bash
sha256sum -c bevy-mcp-linux-x86_64.tar.gz.sha256
```

## Game integration is still required

The prebuilt executable is the persistent supervisor/control plane. Your Bevy game still needs the matching `bevy-mcp` host integration so the supervisor can inspect and control the running ECS.

For development from the current branch, keep the host crates pinned to the same release tag or source revision as the downloaded supervisor. See [Quick Start](../QUICKSTART.md) and [Supervised mode](supervised-mode.md) for the game-side setup and MCP client configuration.

## Building from source

If your platform is not covered by a prebuilt archive, clone the repository and build the supervisor directly:

```bash
cargo build --locked --release -p bevy-mcp-supervisor
```

The resulting executable is written under `target/release/`.
