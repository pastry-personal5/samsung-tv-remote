# P2-M7: Headless Control Runtime

Status: Done (2026-11-12), under the authorized entry exception

## Authorized entry exception

On 2026-11-12 the owner explicitly authorized implementing M7 against the
M6 documented contracts with fake credentials while leaving M6's synthetic
Keychain experiment and final process choice open. This is a phase-plan
exception, not M6 acceptance. The operation/admission contract, existing
storage identities, and fake-port boundaries are the implementation basis.
Signed identity, real credential migration, and background-process evidence
remain deferred to their recorded platform gates.

## Goal

Make the application workflow executable without an Iced event loop so the
future server can own it. One headless runtime owns application decisions,
effects, and results. Keep the GUI working through a temporary in-process
adapter while each behavior moves.

## Entry conditions and boundaries

The [M6 acceptance record](milestone-06-acceptance.md)
still has an open synthetic Keychain experiment and process-choice gate.
Reviewing this plan does not complete M6 or authorize skipping its acceptance.
Before implementation starts, record predecessor acceptance or an explicit
phase-plan exception identifying the stable contracts and deferred evidence.

M7 delivers application runtime behavior and deterministic evidence. HTTP,
authentication/bootstrap, endpoint files, a server executable, detached
launch, and sticky Stop/Resume belong to M8 onward. M7 supplies the runtime
drain interface those lifecycle features will call. Its temporary GUI still
runs the runtime in the GUI process; surviving GUI process exit is M9 work.

## Scope

- Move restore, discovery, probe, Pairing/re-pair, selection, connect,
  disconnected Power Toggle, Wake/cancel, Wake configuration, forget, session
  observation, and dispatch scheduling/completion handling out of Iced.
- Make coordinator state private behind typed commands, snapshots, and
  outcomes. Retain one selected TV and a bounded command dispatcher.
- Add client-scoped operation admission/deduplication, bounded retained
  outcomes, revisioned event fan-out, and atomic subscription. Keep immutable
  snapshots and operation lookup read-only, with explicit unknown outcomes
  after restart or loss of retention.
- Move synchronous storage work off the state owner. Implement durable setup
  recovery, persistence fences, deadlines, and bounded effect supervision.
- Migrate GUI intents and projections to the same typed service through the
  temporary adapter, preserving local drafts, navigation, and status wording.

The [M6 baseline](milestone-06-baseline.md) defines parity and file identities;
the [contract](milestone-06-contract.md) defines preconditions and outcomes.
The architecture below identifies intentional changes from the current code
and the evidence required before publishing the API in M8. No new TV feature
or physical-state observation is added.

## Completion checklist

- [x] A Tokio test harness starts/restores the runtime, submits every scoped
  operation through fake ports, and drains it without initializing Iced.
  Startup does no TV I/O or destructive duplicate-record cleanup; corrupt or
  unavailable stores and pending recovery markers produce visible blocks.
- [x] Commands, queries, receipts, snapshots, and events have application
  types with no presentation, HTTP, MCP, or macOS imports. Mutable coordinator
  fields, session handles, and effect plans are private to the runtime.
- [x] Atomic admission checks epoch and client-scoped keys in contract order,
  retains policy failures, rejects changed payloads, and returns known
  duplicates even at registry/effect/FIFO capacity. One admitted operation
  reserves its outcome before any effect and has exactly one terminal result.
- [x] The sole remote FIFO rechecks target/generation, Pairing, and connection
  immediately before writes. Tests cover same-ID reselect, two-client
  compare-and-swap selection, cancelled waiting work, stale sessions, and
  uncertain in-flight writes without replay.
- [x] Disconnected Power Toggle remains one operation: reachable means ready
  without a power key or packet; only offline/timeout permits one Wake packet.
  Play/Pause resolves at dispatch from runtime-owned inference. Wake captures
  its active configuration at admission and preserves packet facts on cancel.
- [x] Durable selection and configuration publish new generations/revisions
  only after persistence. Pairing checks freshness again before commit;
  journaled Pairing/re-pair/forget recovery is tested at every crash boundary.
  A late blocking worker cannot overwrite newer state or release a fence early.
- [x] Atomic snapshot/subscription, consecutive global revisions, safe
  cross-client projections, overflow/resnapshot, disconnect, retention expiry,
  credential-identity continuity, and restart uncertainty have deterministic
  tests. Observer acknowledgments never remove retained results.
- [x] Fake-time and saturation tests prove responsive queries/drain/completions
  under client load, finite operation lifetimes, bounded workers/handles/
  subscribers/results, and no resource growth from abandoned callers. Record
  measured memory and stage timings; resolve every remaining numeric limit
  in the contract before the M8 handoff.
- [x] GUI buttons, shortcuts, setup, automatic Wake saves, launch reconnect,
  and result wording use the temporary service adapter. It holds only a
  projection and client requests; TV effects continue without GUI polling.
  Closing Settings or losing a subscription does not cancel admitted work.
- [x] The architecture's verification matrix has test/evidence references;
  all [required Cargo checks](../contribution-guide.md#required-validation)
  pass. Update affected current architecture, migration, and contract docs
  to describe what actually landed before marking M7 Done.

The [completion review](milestone-07-review.md) maps every checklist item to
concrete automated evidence and records measured resource/timing limits.
Native signed identity, production Keychain and physical TV evidence remain
at their recorded platform gates; they are not inferred from these tests.

## M8 handoff

Deliver a narrow service handle, runtime task/lifetime and drain handles,
typed errors and operation metadata, atomic subscriptions, fake-port test
fixtures, and measured limits/timings. Record contract changes in the phase
changelog and update affected API examples and MCP corpus before publication.
M8 adds transport and process policy around this same runtime and uses
isolated storage and fake credentials until M9's atomic GUI cutover.

## References

- [Architecture](milestone-07-architecture.md)
- [M6 acceptance and test ownership](milestone-06-acceptance.md)
- [M6 control contract](milestone-06-contract.md)
- [Phase 1 baseline, migration map, and recovery contract](milestone-06-baseline.md)
- [Target architecture](target-architecture.md)
- [Control API Server internals](control-server-internals.md)
