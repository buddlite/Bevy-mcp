# Agent Adapter Checklist

bevy-mcp works with generic reflected ECS state, but a few explicit registrations give agents a much more stable vocabulary for setup, testing, recovery, and causal debugging.

A useful first adapter should register four things:

1. one **semantic action** for stable high-level intent;
2. one **typed Bevy state** for readable/controllable game mode;
3. one **checkpoint resource** for deterministic restore coverage; and
4. one **system-access specification** for exact writer/read attribution on an important system.

## Minimal example

```rust
use bevy::prelude::*;
use bevy_mcp_host::{McpAgentAppExt, McpSystemAccessSpec};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(States, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
enum GameState {
    #[default]
    Menu,
    Running,
}

#[derive(Resource, Default, Serialize, Deserialize)]
struct Economy {
    credits: u64,
}

#[derive(Component)]
struct Cargo;

fn install_agent_adapter(app: &mut App) {
    app.register_mcp_action(
        "give_credits",
        "Add credits to the player for agent setup/testing",
        |world, args| {
            let amount = args["amount"]
                .as_u64()
                .ok_or_else(|| "amount must be an unsigned integer".to_string())?;
            world.resource_mut::<Economy>().credits += amount;
            Ok(json!({ "granted": amount }))
        },
    )
    .register_mcp_state::<GameState>(
        "game_state",
        "Current top-level game state",
    )
    .register_mcp_checkpoint_resource::<Economy>(
        "economy",
        "Economy state restored by MCP checkpoints",
    )
    .register_mcp_system_access(
        McpSystemAccessSpec::new("cargo_transfer")
            .schedule("Update")
            .write::<Cargo>()
            .write_resource::<Economy>(),
    );
}
```

Call `install_agent_adapter(&mut app)` after the relevant state/resources are initialized.

## What each registration unlocks

| Registration | Agent benefit |
| --- | --- |
| `register_mcp_action` | Stable high-level operations for playtests/replay instead of brittle input sequences |
| `register_mcp_state` | `state_get` / `state_transition`, state evidence, and readable playtest setup |
| `register_mcp_checkpoint_resource` | Explicit deterministic checkpoint/restore coverage for that resource |
| `register_mcp_system_access` | Exact declared component/resource access for causal debugging; otherwise writer tools may only have conflict-candidate evidence |

## Recommended progression

Start with reflection and `capabilities`. Add semantic actions for repeated setup operations, then register the small set of state/resources that must survive checkpoint restore. Instrument only the systems where exact writer attribution is worth maintaining; bevy-mcp intentionally labels weaker conflict evidence rather than pretending Bevy exposes universal writer provenance.

The goal is not to mirror every game API into MCP. Register the few concepts that make autonomous tests stable and recovery deterministic.
