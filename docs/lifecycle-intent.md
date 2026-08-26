# Lifecycle intent and passive status

`development_status` is diagnostic. Polling it must never authorize a lifecycle mutation.

For passive `stopped`, `game_exited`, and `idle` states, the recovery contract returns `await_explicit_launch` with no tool recommendation. Starting or rebuilding the game requires an explicit `process_launch`, `process_restart`, or `rebuild_restart` request.

Recovery entries also expose `automatic_safe`:

- `true` means repeatedly following the recommendation is observational/diagnostic only.
- `false` means the suggested action can mutate build or game lifecycle state and requires explicit agent/user intent.

This prevents autonomous harnesses from turning status polling into an accidental watchdog that continuously relaunches a game after it is closed.
