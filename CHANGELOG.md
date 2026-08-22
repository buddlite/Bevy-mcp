# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-06-15

### Added

- Initial release
- 61 MCP tools for Bevy game engine control
- ECS inspection (world_summary, entity_query, entity_get, component_get, component_schema)
- ECS mutation (entity_spawn, entity_despawn, component_insert, component_update, component_remove)
- Resource inspection and mutation (resource_list, resource_get, resource_schema, resource_update)
- Runtime control (launch, stop, restart, pause, resume, step, time_scale)
- Input injection (keyboard, mouse, gamepad, actions)
- UI interaction (query, inspect, click, type)
- Camera control (inspect, set_transform, look_at, frame_entity)
- Screenshot capture (capture_game, capture_camera)
- Asset management (list, get, status, reload)
- Event observation
- Log capture and diagnostics
- Build tools (cargo check, build, test) with structured output
- Playtest framework with assertions
- Batch operations with atomic, dry_run, and verify modes
- Permission system (Read, Write, Full levels)
- Deferred command architecture for safe ECS mutation
- Reflection-based component reading and writing
- Hierarchy tools (tree view, reparent, duplicate)
- Plugin detection
- Operation tracking for async operations

### Architecture

- `bevy-mcp-core` — shared protocol types (no Bevy dependency)
- `bevy-mcp-server` — MCP server over stdio (no Bevy dependency)
- `bevy-mcp-host` — Bevy plugin bridging MCP and ECS

[0.1.0]: https://github.com/buddlite/Bevy-mcp/releases/tag/v0.1.0
