# Debugging intelligence and deterministic replay

This layer adds four high-value capabilities for agent-driven Bevy development.

## Concurrent MCP transport

All legacy, advanced, and debugger requests share one `McpResponseDispatcher`. It is the only consumer of the result queue and routes results by request ID to one-shot channels. Independent MCP calls no longer need a global serialization mutex.

The dispatcher identifies responses by request ID, so out-of-order results are delivered to the correct caller. Tool-level timeouts remove abandoned waiters without blocking other in-flight calls.

## Runtime-to-system causality

Bevy 0.19 keeps the initialized per-system access set private, so the MCP uses a hybrid causal index instead of relying on private fields or unsafe layout assumptions. Register important systems when exact read/write attribution is required:

```rust
app.register_mcp_system_access(
    McpSystemAccessSpec::new("combat::apply_damage")
        .schedule("Update")
        .read::<DamageEvent>()
        .write::<Health>()
        .write_resource::<CombatStats>(),
);
```

`system_access`, `component_writers`, and `resource_writers` return these declarations as `registered_exact`. For systems that are not registered, the MCP also uses Bevy's public schedule conflict graph to return `conflict_candidate` evidence. Conflict candidates are deliberately labelled as incomplete: either side of a conflict may be the writer, and a sole writer with no conflicting system cannot be inferred from that graph.

This narrows a runtime symptom to likely source systems without claiming exact per-frame write provenance. Exact "system X performed this write on frame Y" attribution would require runtime instrumentation around the system execution itself.

## Scoped change tracking

`tracking_config` accepts `mode: "full" | "scoped"`, history length, component/resource allowlists, and exclusions. Full mode preserves existing behavior. Scoped mode retains spawn/despawn tracking but only snapshots change ticks for subscribed component/resource types. Debugger watchpoints and playtest conditions automatically add relevant dynamic subscriptions.

Dynamic subscriptions are reconciled against the currently enabled watchpoints and running playtests after each debugger tick. Once a one-shot watchpoint fires, a watchpoint is removed, or a playtest completes/cancels, interests that are no longer required are removed rather than accumulating for the rest of the process. In scoped mode, tracker snapshots are reset only when the effective dynamic-interest set actually changes.

Example:

```json
{
  "mode": "scoped",
  "history_frames": 300,
  "components": ["Health", "Cargo"],
  "resources": ["Economy"],
  "exclude_components": ["Transform"]
}
```

## Deterministic checkpoints and replay

Arbitrary Bevy worlds cannot be safely cloned while preserving every entity identity and non-reflected engine object. Checkpoints are therefore explicit: games register deterministic resources or custom adapters.

```rust
app.register_mcp_checkpoint_resource::<SimulationRng>(
    "simulation_rng",
    "RNG state used by deterministic simulation",
);
```

Checkpoint restore is transactional across the adapters covered by the checkpoint. Before applying a restore, the registry validates that every required adapter still exists and captures rollback values for every covered adapter. If an adapter fails—even after partially mutating its own state—the registry attempts to restore the failing adapter and every already-applied adapter. If rollback itself fails, the returned error identifies those failures and callers must treat the affected registered state as uncertain.

Then an agent can:

1. `checkpoint_create`
2. `recording_start`
3. invoke semantic actions / state transitions / debugger key steps
4. `recording_stop`
5. change code or simulation state
6. `replay_start` with the checkpoint ID
7. poll `replay_status`

Replay preserves the original frame offsets between recorded actions. `replay_start` preflights the recording ID and concurrent-replay constraint before restoring a checkpoint, so an invalid replay request does not first mutate deterministic checkpoint state. The replay start path validates again when the replay runtime is created.

Checkpoint coverage is returned by `checkpoint_list` so agents can tell exactly what state is deterministic rather than assuming the whole engine was snapshotted. The transactional guarantee applies only to the explicitly registered checkpoint adapters; unregistered Bevy world state remains outside checkpoint coverage.
