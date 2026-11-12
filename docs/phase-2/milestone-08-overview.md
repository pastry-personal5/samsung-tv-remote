# P2-M8: Control HTTP Server

Status: Planned

## Goal

Run the headless control service as a separate background process and expose
all supported TV setup and control operations through a local, versioned JSON
HTTP API with an event stream.

## Scope

- Add a server binary, per-user single-instance behavior, on-demand launch
  primitive, loopback listener, authenticated readiness, and clean shutdown.
- Implement `/v1` state, saved-TV, discovery/probe, Pairing, selection,
  connect, configuration, remote-action, Wake, operation-result, forget, and
  event contracts fixed in P2-M6.
- Protect both read and write access; bound inputs, requests, event backlog,
  concurrent effects, and retained operation results.

## Completion checklist

- [ ] The server starts without Iced, restores saved TV and trust data, and
  owns the only TV session and Keychain access for its profile. Process
  startup alone does not discover, pair, connect, Wake, or send remote keys.
- [ ] Two authenticated HTTP clients see the same selected-TV snapshot and
  ordered events; stale selection and duplicate operation keys cannot send
  the wrong or repeated command within the retention window. A changed
  server epoch rejects a stale mutation.
- [ ] Both protocol handlers use the same typed service façade. The HTTP
  adapter has no direct TV/storage port, and application-level rejection or
  an overloaded ingress cannot create an untracked side effect.
- [ ] Loopback-only bind, Host and Origin policy, client authentication,
  size limits, and safe error/log handling have deterministic HTTP tests.
  The event stream starts with an atomic snapshot and never silently misses
  the gap before live events.
- [ ] A second instance detects the first safely; port collision with an
  unrelated process fails closed. The server remains running after a client
  disconnects. A user-stopped marker is distinct from crash and prevents
  reconnect loops from restarting the server.
- [ ] Endpoint allocation persists a non-secret per-user MCP URL and rejects
  a stolen port; runtime files are owner-only and atomically updated. Tests
  cover stale manifests, simultaneous starts, incompatible build/protocol
  readiness, fast user switching, logout, sleep/wake, and an unavailable or
  locked credential store.
- [ ] Development runs use fake TV and credential ports plus isolated
  non-secret storage while the shipped GUI still owns its local runtime.
  An alternate preferences directory alone is not treated as Keychain
  isolation; no development run reads production Pairing items.
- [ ] Fake TV, Wake, and Pairing tests cover every API operation. Required
  Cargo gates pass against committed configuration. The contribution guide
  documents explicit GUI/server development commands, and adding the server
  binary does not silently break its existing `cargo run` guidance.

## References

- [M6 acceptance and test ownership](milestone-06-acceptance.md)

- [Architecture](milestone-08-architecture.md)
- [Target architecture](target-architecture.md)
- [Lifecycle and security research](server-lifecycle-security.md)
- [Control API Server internals](control-server-internals.md)
