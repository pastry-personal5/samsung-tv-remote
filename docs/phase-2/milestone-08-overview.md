# P2-M8: Control HTTP Server

Status: Done

## Goal

Expose the headless control runtime through authenticated loopback HTTP and
ordered events. Run it on demand as a separate background process using the
same signed app executable in `--server` mode.

## Owner decisions and entry

The owner confirmed these decisions on 2026-11-12:

- Start isolated HTTP and process work with fake credentials now. M7 is Done
  and supplies the typed runtime, fake-port fixtures and measured limits.
- Use the same signed executable for GUI and `--server` processes. Dispatch
  server mode before Iced initialization. Normal `cargo run` opens the GUI.
- Start on demand, remain available after GUI exit, never start at login,
  and honor sticky Stop until explicit GUI Resume.
- Start with a fresh, empty device profile. Import no old devices, selection,
  trust, recovery markers or pairing credentials. Use distinct storage and
  credential namespaces. Later launches restore devices added to this profile.
- Review interview: keep the early probe in the existing GUI with minimal
  wiring; show an incompatible-build error and require explicit restart;
  close existing event streams when API credentials rotate, then authenticate
  again and take a fresh snapshot without replaying mutations.
- GUI server connection/reconnection runs automatically in the background:
  immediate first attempt, then 1-second exponential delays capped at one hour,
  continuing while open. Sticky Stop and incompatible builds retain their
  explicit Resume/restart requirements; connection retries never replay actions.
- No legacy device-key migration or synthetic legacy migration experiment is
  required. Existing local data is not deleted by this plan.
- Deliver all three outcomes: complete API coverage with deterministic tests,
  runnable background lifecycle, and an early isolated GUI connection proving
  the process/API boundary.

This is the recorded M8 entry exception; M6 final acceptance is not inferred.
The removed migration experiment is no longer a prerequisite. Signed checks
for newly created Keychain credentials remain M9 evidence.

## Scope and milestone boundaries

M8 implements `/v1` state, saved-TV reads, discovery/probe, Pairing/re-pair,
selection, connect, Wake configuration, remote actions, Wake/cancel, operation
lookup, explicit interrupted-setup recovery, events and GUI-authorized Stop.
Both reads and writes use the shared access gate and the existing typed service.
The server owns its profile's only runtime and TV session.

Development runs use fake TV, Wake, discovery and credential ports plus isolated
non-secret storage. A different preferences directory alone does not isolate
Keychain. M8 never opens production pairing items or contacts a real TV.

An explicit development GUI mode attaches to this isolated server without
constructing the GUI's in-process TV runtime. The early probe covers attach,
initial state/events, one tracked remote action after fake setup, disconnect,
and reattach. It demonstrates GUI/server independence; complete shipping GUI
workflow migration and real signed first-use setup belong to M9. The default
GUI remains on its existing runtime until that cutover.

M8 supplies bootstrap/lifecycle ports, capability enforcement, fake credential
behavior and executable process tests. Signed peer identity and fresh-credential
Keychain checks belong to M9. `/mcp` and the Claude header helper belong to M10;
M8 supplies their shared gate and stable URL. Full signed upgrade, logout,
sleep/wake and fast-user-switch integration belongs to M11.

## Completion checklist

- [x] `--server` enters a headless composition path in the same executable.
  Empty first launch performs no legacy reads, discovery, Pairing, connect,
  Wake or remote action. New-profile restart restores only that profile.
- [x] All scoped routes and recovery operations map to the typed service.
  Handlers have no direct TV/storage access. Every operation and typed outcome
  has deterministic HTTP fixtures and fake-effect assertions.
- [x] Two authenticated clients share selected-TV state and ordered events.
  Stale epoch/generation and duplicate/changed keys preserve M7 admission,
  retention and uncertainty semantics, including response loss and overload.
- [x] Readiness, reads, mutations and events enforce loopback, Host, Origin,
  authentication, capability and input policies. GUI/MCP identities remain
  stable across rotation; revocation prevents admission and closes streams.
- [x] SSE starts with an atomic snapshot, preserves consecutive revisions and
  cross-client privacy, and requires resnapshot after overflow or disconnect.
- [x] On-demand detached start/attach, parent-exit survival, single-instance
  ownership, persistent endpoint allocation and authenticated readiness pass
  subprocess tests. Stale manifests, unrelated port occupants and incompatible
  builds fail safely without a second owner or silent endpoint change.
- [x] Sticky Stop is durable before acknowledgment; Resume is explicit and
  GUI-authorized. Crashes, reconnect, signals and late blocking workers preserve
  outcome facts and profile ownership. Client disconnect never stops the server.
- [x] Authorization and owner admission have a defined ordering: a request
  queued before revocation cannot start afterward unless already admitted.
  Rotation closes streams; reconnect preserves client identity and never replays.
- [x] Lifecycle tests cover the mutation-admission fence, crash before/after
  durable Stop, lost Stop acknowledgment, explicit incompatible-build restart
  and Resume waiting for settlement/lock release without deadlock.
- [x] Runtime files are owner-only and atomically updated. Fake bootstrap tests
  cover peer rejection, replay and blocked credential stores. Signed identity
  evidence is clearly assigned to M9 rather than claimed from fakes.
- [x] The development GUI probe attaches without a local TV runtime, receives
  state/events, submits and looks up a tracked action, and reattaches while
  server state survives its exit. Default GUI behavior remains intact.
- [x] The GUI automatically connects/reconnects in the background with one
  bounded attempt at a time, short initial delays and exponential backoff capped
  at one hour. Fake-clock tests cover cap/reset, blocked states, sleep and exit;
  subprocess tests prove outage recovery without replaying mutations.
- [x] Transport limits and worst-case profile JSON/SSE overhead are measured.
  Slow or abandoned connections cannot grow buffers without bound or cancel
  admitted work. Readiness, queries and drain progress under load.
- [x] Fake profiles cannot be opened with production adapters or promoted to
  the production namespace. A paired fake profile survives a server-process
  restart and supports explicit reconnect/action with synthetic credentials.
- [x] Resolve wire IDs, lossless integers, recovery routes, generation/cancel fields, expiry
  metadata and outcome mapping before publishing `/v1`. Update the contract,
  examples and MCP corpus, including fresh-profile first-use behavior.
- [x] Required Cargo checks pass; the contribution guide documents tested GUI,
  fake-server and GUI-probe commands. The M8 review maps every checklist item
  to evidence and names remaining signed integration work.

## Delivery order

Follow the [architecture sequence](milestone-08-architecture.md#implementation-sequence):
contract/fresh-profile composition; access/process shell; an early GUI/API
slice; complete API/events; lifecycle/load hardening; review and handoff.
All three requested outcomes are required for completion. The
[implementation record](milestone-08-implementation.md) records the executable,
client, native capture and measured load evidence. The
[acceptance audit](milestone-08-review.md) maps every checklist item and names
the HTTP fault, crash-boundary and documentation consistency evidence.

## References

- [Completion review](milestone-08-review.md)
- [Original plan review](milestone-08-plan-review.md)
- [Architecture](milestone-08-architecture.md)
- [M6 acceptance and owner decisions](milestone-06-acceptance.md)
- [M7 runtime evidence](milestone-07-review.md)
- [Target architecture](target-architecture.md)
- [Lifecycle and security research](server-lifecycle-security.md)
- [Control API Server internals](control-server-internals.md)
