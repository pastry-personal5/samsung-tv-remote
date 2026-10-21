# P2-M7: Headless Control Runtime

Status: Planned

## Goal

Make the application workflow executable without an Iced event loop so the
future server can own it. Keep the GUI working through a temporary local
adapter while each behavior moves.

## Scope

- Move effect scheduling and completion handling for discovery, probe,
  Pairing, reconnect, Wake, forget, and dispatch out of Iced.
- Make coordinator state private behind typed commands, snapshots, and
  outcomes. Retain one selected TV and a bounded command dispatcher.
- Add an application event stream and operation lookup suited to multiple
  clients, with a restart epoch, selection generation, bounded retention,
  and explicit unknown outcomes after restart.
- Define application command handlers separately from Iced messages and
  display models; keep queries for snapshots and results read-only.

## Completion checklist

- [ ] The control runtime starts, restores saved state, and handles every
  Phase 1 operation with fake adapters and no presentation code.
- [ ] Application commands and queries use typed inputs/results with no Iced,
  HTTP, or MCP types. The temporary GUI adapter maps existing messages into
  them without creating another authority or remote-action queue.
- [ ] Commands are serialized by the authoritative runtime. Queue admission
  rechecks selection and pairing before writes; cancelled waiting actions are
  not sent and interrupted writes remain uncertain.
- [ ] Selection changes use an expected generation. Remote, Wake, and both
  Power Toggle paths validate device ID, epoch, and generation; disconnected
  Power Toggle and Play/Pause inference remain server-owned.
- [ ] Snapshot and revision tests cover atomic subscriber attachment,
  concurrent observers, gaps, slow observers, disconnect, restart, stale
  completion, and result expiry. One observer cannot consume another's
  terminal result.
- [ ] The state owner stays responsive while TV/storage effects are pending;
  completion delivery has reserved capacity under client load. Selection
  persistence, late Pairing, partial forget, and shutdown preserve the
  server-internal ownership and outcome rules.
- [ ] Existing GUI behavior remains usable through the temporary adapter;
  the full Cargo gates pass before the HTTP server work begins.

## References

- [Architecture](milestone-07-architecture.md)
- [Target architecture](target-architecture.md)
- [Control API Server internals](control-server-internals.md)
