# MCP client configuration samples

The JSON files in this directory use the persistent `bevy-mcp` **supervisor** command.

For a project-local MCP configuration, run the client from the game project directory or add an explicit project path:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/bevy-mcp",
      "args": ["--project-dir", "/absolute/path/to/my-bevy-game"]
    }
  }
}
```

The supervisor can discover a single Cargo binary automatically. Multi-binary workspaces should provide `package` and/or `bin` to build/rebuild tools. See [supervised mode](../docs/supervised-mode.md) for the complete contract.

The client guides under `docs/guides/` document the alternative embedded mode.
