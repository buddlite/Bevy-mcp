# Using bevy-mcp with Local LLMs (Ollama & LM Studio)

Run bevy-mcp with a local model — no API key, no cloud costs, fully offline.

> This guide targets the current `v.01` development surface. Follow the root Quick Start for matching source dependencies and `AgentBevyMcpServer` integration before configuring a local-model client.

---

## Why Use Local Models?

- **Free.** No per-token charges. Run as many queries as you want.
- **Offline.** Works without internet. Great for travel, restricted networks, or privacy-sensitive projects.
- **Private.** Your game code and ECS data never leave your machine.
- **Fast iteration.** Local models respond quickly for simple queries like entity lookups and component reads.

### Limitations vs Cloud Models

- **Smaller context windows.** Local models (7B–70B parameters) handle shorter conversations than Claude or GPT-4. Complex multi-step workflows may lose context.
- **Less reliable tool calling.** Local models are improving but still more likely to hallucinate tool names or produce malformed arguments. Use structured prompts.
- **Slower for large models.** A 70B model on a single GPU may take 10–30 seconds per response. Smaller models (7B–13B) are faster but less capable.
- **No multi-modal support (mostly).** Screenshot analysis requires vision models (LLaVA, etc.) which are less mature.

---

## Option A: Ollama

[Ollama](https://ollama.com) is the easiest way to run local models. It handles model downloads, quantization, and serving.

### Install Ollama

**macOS / Linux:**

```bash
curl -fsSL https://ollama.com/install.sh | sh
```

**Windows:**

Download from [ollama.com/download](https://ollama.com/download).

### Pull a Model

```bash
# Best for tool calling (recommended)
ollama pull qwen2.5:14b

# Faster, smaller, less reliable with tools
ollama pull llama3.1:8b

# Larger, more capable, slower
ollama pull qwen2.5:32b

# With vision (for screenshot analysis)
ollama pull llava:13b
```

### Configure bevy-mcp with Ollama

You need an MCP-compatible client that supports local models. The most common setup is with **Cline** in VS Code, which can use Ollama as its LLM backend. See the [Cline guide](cline.md) for general Cline setup details.

#### Step 1: Set up bevy-mcp in your Bevy project

Same as any other agent — add the dependency and plugin (see the [main README](../../README.md#quick-start)).

#### Step 2: Build your game

```bash
cargo build
```

#### Step 3: Configure Cline to use Ollama + bevy-mcp

In VS Code with Cline:

1. Open Cline settings (gear icon in the Cline panel)
2. Set **API Provider** to **Ollama**
3. Set **Base URL** to `http://localhost:11434`
4. Set **Model** to `qwen2.5:14b` (or your preferred model)
5. Go to **MCP Servers** and add bevy-mcp:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/your-game/target/debug/your-game-name",
      "args": []
    }
  }
}
```

6. Start a conversation and try: *"Check the health of the Bevy game"*

---

## Option B: LM Studio

[LM Studio](https://lmstudio.ai) is a desktop app for running local models with a GUI. It exposes an OpenAI-compatible API that MCP clients can use.

### Install LM Studio

Download from [lmstudio.ai](https://lmstudio.ai) and install.

### Download a Model

1. Open LM Studio
2. Go to the **Search** tab
3. Search for one of these models:
   - `qwen2.5-14b-instruct` (best for tool calling)
   - `llama-3.1-8b-instruct` (fast, lightweight)
   - `mistral-7b-instruct` (compact alternative)
4. Click **Download**

### Start the Local Server

1. Go to the **Local Server** tab in LM Studio (the `<->` icon)
2. Select your downloaded model
3. Click **Start Server**
4. The server runs at `http://localhost:1234` by default
5. Enable **Apply Prompt Template** for best results

### Configure bevy-mcp with LM Studio

LM Studio exposes an OpenAI-compatible API. Use it with Cline in VS Code:

1. Open Cline settings
2. Set **API Provider** to **OpenAI Compatible**
3. Set **Base URL** to `http://localhost:1234/v1`
4. Set **API Key** to `lm-studio` (any string works)
5. Set **Model** to the model you loaded in LM Studio
6. Go to **MCP Servers** and add bevy-mcp:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/your-game/target/debug/your-game-name",
      "args": []
    }
  }
}
```

7. Start a conversation and try: *"Check the health of the Bevy game"*

---

## Verify the Connection

In Cline, click the **MCP Servers** icon (plug icon). You should see `bevy` listed with a green status indicator. If it shows red or is missing, see Troubleshooting below.

---

## Recommended Models for Game Dev

| Model | Size | Tool Calling | Speed | Best For |
|---|---|---|---|---|
| **Qwen 2.5 14B** | 14B | Excellent | Medium | General game dev — best balance |
| **Qwen 2.5 32B** | 32B | Excellent | Slower | Complex reasoning, multi-step tasks |
| **Llama 3.1 8B** | 8B | Good | Fast | Quick queries, entity lookups |
| **Mistral 7B** | 7B | Fair | Fast | Lightweight tasks, limited VRAM |
| **LLaVA 13B** | 13B | Limited | Medium | Screenshot analysis (vision model) |

> **Minimum VRAM:** 8GB for 7B models, 12GB for 14B models, 24GB for 32B models. Use quantized versions (Q4_K_M) to reduce memory requirements.

---

## Tips for Local Models

- **Keep prompts short.** Local models have smaller context windows. Be specific: *"call health"* instead of *"please check if the game is running and tell me about the entity count and frame number and whether it's paused"*.
- **One tool call per message.** Local models handle single tool calls more reliably than multi-step plans. Ask for one operation at a time.
- **Use Qwen 2.5 for tool calling.** It has the best MCP tool-calling support among open models as of 2025. Other models may misformat tool arguments.
- **Lower temperature for tool calls.** Set temperature to 0–0.1 when making MCP tool calls. Higher temperatures cause creative but invalid tool arguments.
- **Upgrade model size for complex workflows.** If you need multi-step entity manipulation (query → filter → update → verify), use a 14B+ model.
- **Combine with cloud fallback.** Use local models for simple queries (health, entity_query, component_get) and switch to a cloud model for complex reasoning.

---

## Troubleshooting

- **Binary not found / path errors:** Ensure the path in the MCP config is the absolute path to your compiled game binary, not the source directory. Check that the binary exists after `cargo build`.
- **MCP server not appearing:** Verify the binary compiles and runs standalone first — `cargo build`, then run `target/debug/your-game-name` directly in a terminal.
- **Tools not showing up:** Restart your MCP client (Cline, etc.) after changing MCP config. Some clients cache the tool list.
- **Permission errors:** The default permission is `read_only()`. If mutation tools are missing, upgrade to `McpPermissions::write()` or `McpPermissions::full()`.
- **Game crashes on startup:** Run the binary directly in a terminal to see error output — the MCP client swallows stderr from the subprocess.
- **Local model not calling tools:** Use Qwen 2.5 for best tool-calling support. Lower temperature to 0–0.1 for more reliable tool invocations.
