# Phase 2 decision log

Status: Active

- 2026-11-12 — Follow-up M8 code review fixed cancelled submissions losing
  lookup recovery, SSE idle deadlines resetting on read cancellation, slow
  lookups blocking event consumption and transient subscription failures
  permanently blocking reconnect. Added cancellation, heartbeat/idle and HTTP
  status regressions. All recovery remains lookup-only; no mutations replay.
  See the [review findings](milestone-08-review.md#follow-up-code-review-2026-11-12).

- 2026-11-12 — Completed P2-M8 under the recorded fake-profile entry exception:
  authenticated full `/v1` HTTP/SSE, same-executable detached launch, protected
  listener proof, sticky Stop/explicit Resume/restart and the existing-GUI
  development probe. Added actual HTTP fault/load and process/crash/rotation
  fixtures, strict non-promotable development documents, measured overlapping
  profile output, native capture and executable contract/corpus checks.
  [Review](milestone-08-review.md) maps all sixteen gates; captured Cargo
  validation passes. M9 retains signed peer/fresh-Keychain and full GUI cutover;
  M10 retains MCP and real token budgets; M11 retains signed session integration.

- 2026-11-12 — Added M8's authenticated loopback HTTP/SSE library, guarded
  routing and real HTTP fake-effect fixtures. Added the owner admission fence,
  durable Stop actor, failure/lost-reply retention and lock settlement tests;
  captured stable terminal generation/configuration facts. Reserved query and
  management capacity under slow mutation and failed-auth traffic. Executable
  bootstrap, detached lifecycle, Resume and GUI integration were still open
  at that initial slice; the completion record above supersedes that status.

- 2026-11-12 — Expanded the P2-M10 plan with predecessor entry gates,
  protocol/catalog boundaries, explicit 13-tool service mappings and pending
  schema reconciliation, operation cancellation/recovery, signed same-executable
  header helper and opt-in Claude Code configuration. Added delivery slices,
  deterministic and installed-client evidence ownership, and actual token/call
  acceptance gates. M10 remains Planned; no client configuration, implementation
  or signed-platform acceptance is claimed.

- 2026-11-12 — Started M8 implementation: strict request/wire codecs, safe
  outcome projections, private atomic operation references, stable display
  expiry, owner-ordered authorization revocation, shared access policy, secure
  runtime-file/lock primitives and fake-clock reconnect policy. Added deterministic
  fixtures and corrected the no-packet Power readiness projection. M8 remains
  In progress; HTTP serving, process lifecycle and GUI integration remain open.

- 2026-11-12 — Expanded the P2-M9 plan against the current in-process GUI and
  partial M8 wire work. Specified M8 entry/release gates, client/projection
  ownership, request identity and lost-response recovery, external-selection
  and draft conflicts, workflow parity, lifecycle UI and signed fresh-profile
  verification. Removed superseded legacy-migration acceptance wording;
  preserved existing legacy data and owner decisions. Added delivery slices
  and an evidence matrix. M9 remains Planned; no implementation, Cargo or
  signed-platform acceptance is claimed.

- 2026-11-12 — Owner requested automatic GUI server connection/reconnection in
  the background, starting at short intervals with exponential backoff capped
  at one hour. Applied to M8's probe and M9's shared client: immediate first
  attempt, then 1-second doubling delays, repeated hourly retries, one bounded
  attempt/timer and reset after 30 healthy seconds. Preserve sticky Stop,
  explicit incompatible-build restart, prompt-free credential gates and no
  mutation replay; require fake-clock and subprocess outage evidence.

- 2026-11-12 — Reviewed M8 against the M7 service and current M6 wire draft.
  Required explicit contract decisions for ID encoding, integer precision,
  stable expiry, recent-operation privacy and conflicting body budgets. Added
  revocation/admission ordering, authenticated attach transcript, Stop crash
  boundaries/lock ordering, fake-profile restart isolation and reserved load
  capacity. Owner interview selected minimal wiring in the existing GUI,
  explicit restart on build mismatch and stream closure on credential rotation.
  [Plan review](milestone-08-plan-review.md) separates planning fixes from
  implementation and signed-platform evidence; M8 remains Planned.

- 2026-11-12 — Consolidated the M8 plan around the owner's decisions:
  same-executable on-demand `--server`, fresh empty profile with distinct
  namespaces, no legacy/synthetic migration, and isolated fake-port entry.
  Made complete API coverage, runnable lifecycle and an early GUI/API probe
  explicit completion requirements and implementation slices. Assigned signed
  fresh-credential/peer verification to M9 and full signed sessions to M11;
  preserved the M6 Host/Origin policy instead of silently tightening it.
  M8 remains Planned; this update implements no server or GUI behavior.

- 2026-11-12 — The owner chose a fresh, empty server device profile with no
  legacy import. Require distinct storage/credential namespaces, empty first
  launch, explicit first Pairing and persistence across subsequent launches.
  Existing local data is not deleted by this planning decision.

- 2026-11-12 — The owner confirmed no device-key migration is needed,
  including synthetic legacy migration. Removed that experiment as an M6
  blocker and removed old-GUI credential migration from the M9 plan. Retain
  fresh-credential signed Keychain lifecycle/prompt/failure verification in M9.
  No existing item deletion, credential transfer or M6 completion is implied.

- 2026-11-12 — The owner selected an on-demand background process using
  the same signed executable in `--server` mode. Updated M6 acceptance and
  M8/target composition plans: dispatch before Iced, retain default GUI launch,
  and use separate composition modules rather than a second server binary.
  Shared code identity does not isolate Keychain access between modes. Signed
  legacy access/migration evidence remains open; M6 is not accepted by this
  decision and production cutover remains M9-gated.

- 2026-11-12 — The owner authorized isolated M8 HTTP work with fake
  credentials while M6's signed Keychain/process gates remain open. Recorded
  the M8 entry exception. Required outcomes include complete deterministic API
  coverage, runnable background lifecycle and an early isolated GUI connection
  probe. Shipping GUI migration and production profile access remain M9 work.

- 2026-11-12 — Expanded the P2-M8 plan against the completed M7 runtime.
  Added contract reconciliation for opaque IDs, recovery and generation
  fields; HTTP/access/bootstrap boundaries; secure endpoint lifecycle,
  Stop/drain settlement; proposed transport budgets, implementation slices
  and deterministic verification ownership. M8 remains Planned. M6's signed
  Keychain/process gates remain open; no M8 entry exception, production
  profile access or signed integration acceptance is inferred from planning.

- 2026-11-12 — Reviewed M7 and fixed concurrent Pair/Wake cancellation,
  terminal connection projections, and successful preference-write profile
  reconciliation. Added six deterministic regression tests; the M7 review
  records the findings and evidence. Platform acceptance gates remain open.

- 2026-11-12 — Completed M7 under the recorded entry exception. All GUI TV
  intents now use one headless service; removed the legacy presentation effect
  runners and mutable public coordinator. Added prepared Pairing commits,
  journaled first-Pairing/re-pair/rotation recovery, client-bound expiring
  candidates/observations, captured-config Wake, bounded worker supervision,
  indexed retained results and GUI resnapshot/result-race handling. Storage
  timeouts retain fences, and a prepared connection cannot install inside an
  uncommitted selection fence. Recovery tests cover 46 interruption points
  across Forget, setup commits and compensation. Joint production-capacity
  heap/stage measurements and the verification matrix are in the M7 review.
  Finalized 10-second storage/send reporting and five-second drain reporting,
  preserved 2-second disconnected Power, 30-second Wake readiness and
  75-second Connect including their storage stages. M6 signed Keychain and
  process-choice acceptance remains open; no HTTP/background server or
  production credential migration is claimed.

- 2026-11-12 — Rewrote the M6 human runbook for a clean restart in the
  `m6probe` desktop account after an existing-installation stop. Main-account
  preparation now initializes the saved signing identity. Reset verifies the
  handoff first, unregisters any optional agent before removal, clears
  disposable Keychain items, removes the test apps/data, and reinstalls before
  a new pairing. Resetting or staging is not human acceptance; M6 stays Active.

- 2026-11-12 — Reviewed M6's by-human scripts and instructions. Added
  `--keychain-only` handoffs so optional LaunchAgent failures cannot block
  required Keychain evidence. Fixed handoff-path scope for refresh, separated
  main/helper identity reporting and staging/build provenance, and clarified
  recovery after a partial token replacement. Extended disposable staging
  fixtures for both modes and argument validation. Human execution and
  process-choice acceptance remain open; M6 stays Active.

- 2026-11-12 — Added journaled headless Forget and exact-target/transaction
  recovery. Startup loads pending cleanup before allowing control and never
  retries credential access on its own. Selection clears durably before
  deletion; partial results retain per-store facts and block the profile.
  Preference and journal renames/removal sync the parent directory. Temporary
  file/fake credential tests interrupt twelve persistence boundaries and
  verify restart/recovery. Added client-scoped operation lookup by ID.
  Pairing/rotation recovery and GUI cutover remain incomplete.

- 2026-11-12 — Added the M7 five-second drain reporting boundary and a
  separate settlement handle. Mutation admission closes while read-only
  queries remain available. Unresolved storage stays supervised/fenced;
  interrupted writes retain uncertain outcomes and queued writes remain
  not-sent. Fake-time tests cover blocked startup, late storage completion,
  pending effects, idle completion and FIFO outcomes. Durable recovery,
  remaining runtime workflows and GUI migration are still outstanding.

- 2026-11-12 — Fixed probe build publication after a handoff selected an
  unfinished LaunchAgent app. Builders now keep temporary bundles outside
  staging discovery and publish only after deep signature verification.
  Staging reports the failed source and rebuild command without substituting
  an older build. Disposable fixtures reproduce the resource-envelope error
  and verify the copy path; no production identity or credential item was used.

- 2026-11-12 — Refreshed the M6 by-human runbook and added a reproducible
  signed-bundle handoff helper. The previous temporary directory was absent;
  existing probe/GUI bundles passed host signature checks and were copied to
  a fresh, test-account-readable directory with a path-filled runbook.
  Required Keychain cases now precede optional LaunchAgent research, with
  explicit results, prompt counts, failure stops and cleanup. Human Keychain
  execution and process-choice acceptance remain pending; M6 is Active.

- 2026-11-12 — Recorded the owner's explicit M7 entry exception while M6's
  Keychain and process gates remain open. Added an initial headless owner,
  typed bounded service, client-scoped retained operations, atomic events,
  asynchronous restore and fenced selection/configuration, plus fake-port
  tests. Session writes progress without GUI polling and uncertain writes
  reset playback inference. M7 remains in progress: setup recovery, Wake,
  GUI migration and measured handoff limits are outstanding.
- 2026-11-11 — Reviewed the M7 plan against current code and the M6 handoff.
  Added the extraction sequence, typed-service and temporary-GUI boundaries,
  admission/retention ordering, storage serialization and crash recovery,
  stage timing/capacity evidence, and a deterministic verification matrix.
  Identified startup duplicate deletion, synchronous owner storage, combined
  approval/commit, and Wake-edit cancellation as explicit migration work.
  M7 remains Planned; M6's open Keychain/process gate is not waived.

- 2026-11-11 — Re-ran M6 completion probes on macOS 27.2.1 with the existing
  local signing identity. Signed lifecycle/collision/Stop/Resume/crash checks
  passed; Claude Code 2.1.322 negotiated the planned revision against the fake
  endpoint. Proxy budgets reproduced and all Cargo gates passed (141 tests).
  Accepted lifecycle and endpoint feasibility for design freeze. Rebuilt and
  verified the signed Keychain probe without accessing an item. The isolated
  legacy-Keychain experiment and dependent process-choice acceptance remain
  open, so M6 stays Active.

- 2026-11-11 — Reviewed P2-M6 with the owner. M6 accepts design and focused
  probes; production verification belongs to M7–M11. Target is the owner's
  Mac with local signing and no notarization. Allow one explicit migration/
  Keychain approval, then zero routine prompts. Keep on-demand launch,
  GUI-close persistence, no login start and sticky Stop/GUI Resume. MCP clients
  own human approval of explicit confirmation steps; TV Pairing approval
  remains mandatory. Added the acceptance/handoff record, clarified retained
  policy failures and supervision deadlines, and removed rejected LaunchAgent
  research from mandatory gates. Synthetic Keychain feasibility and final
  process-choice acceptance remain open; M6 is Active, not Done.

- 2026-10-15 — Reviewed the M6 contract against bounded retention and
  multi-client events. Unretained operation lookup now reports an unknown
  outcome instead of claiming expiry was observed; every subscriber receives
  a safe coarse event for each global revision. Pairing observations bind
  client, host, epoch and generation. Config revisions are epoch-scoped to
  preserve `devices.json` v2. The signed isolated probe now verifies sticky
  Stop, explicit Resume and forced-crash relaunch, without claiming real
  control-operation drain or Keychain migration.
- 2026-10-15 — Began P2-M6 implementation review. Added a concrete draft
  `/v1` and MCP contract, Phase 1 ownership/data migration map, sanitized MCP
  corpus and reproducible proxy budgets, and an isolated signed lifecycle
  experiment. An isolated Claude Code 2.1.300 health check selected the
  2026-07-28 MCP path and ran a fake auth-header helper. Same-ID selection
  now follows the current generation-changing coordinator behavior.
  Keychain, macOS privacy, endpoint, signed-server, and model-context gates
  remain explicitly unverified.
- 2026-10-15 — Added the proposed [Control API Server internal
  architecture](control-server-internals.md). It assigns one state owner for
  command admission, selected-TV/session state, operation retention, and
  event revision; bounded workers return tagged effects. It specifies
  durable-write fences, stale completion handling, factual cancellation,
  typed query/subscription paths, and lifecycle drain order. M6, M7, M8,
  M10, and M11 now link to or verify this design; unproven macOS and numeric
  capacity choices remain milestone gates.
- 2026-10-15 — Added an MCP context-efficiency plan: semantic tools instead
  of per-button or generic batch tools, deterministic cacheable catalog,
  concise summary-first results, bounded operation lookup, and measured
  model-visible token/round-trip budgets for Claude Code with and without
  tool search. Safety-critical IDs, preconditions, confirmations, and
  uncertain outcomes remain explicit; structured output is not assumed to
  save tokens when a compatibility text copy is also exposed.
- 2026-10-15 — Clarified how Clean Architecture, MVVM-inspired Iced
  presentation, and typed command handling fit together. The server remains
  the single TV authority; the GUI uses a view model for local state and
  revisioned server projections, while Iced messages map through a typed
  client to service commands. Added migration and test gates in P2-M6, M7,
  M9, and M11 without adding a command bus, second queue, or full CQRS.
- 2026-10-15 — Added [server lifecycle and security research](server-lifecycle-security.md)
  to P2-M6 through P2-M11. Preferred on-demand signed helper over login
  registration, made GUI close distinct from sticky explicit Stop, and allowed
  a configured Claude Code HTTP connection to start an absent server without
  starting at macOS login. Added a synthetic legacy-Keychain access gate,
  per-user endpoint/collision tests, Local Network/privacy and distribution
  checks, and a token-free Claude Code `headersHelper` integration. Routine
  service management is automatic; OS/TV trust decisions remain explicit.
- 2026-10-15 — Reviewed P2-M6 through P2-M11 for implementable contracts.
  Added atomic snapshot/subscription, selection compare-and-swap, epoch-bound
  operation keys with finite retention and honest crash uncertainty, separate
  Wake and remote-write outcomes, isolated Keychain test ports, explicit MCP
  revision/security checks, and a signed GUI/server cutover in P2-M9 before
  P2-M11 lifecycle hardening. Also assigned two-binary Cargo guidance to M8
  and bundle instructions to M9. These clarify the existing server-first
  choice.
- 2026-10-15 — Replaced the unimplemented conditional local-control plan with
  the server-first [Phase 2 plan](phase-2.md). Preserved its P2-M1 through
  P2-M5 IDs as Dropped and archived the prior plan; new work starts at P2-M6.
  Sources, Apps, and Text Input remain future feature candidates.
- 2026-10-15 — Chose a separate per-user background Control API Server. It
  hosts versioned JSON HTTP and an event stream, plus a distinct `/mcp`
  Streamable HTTP endpoint in the same process. Both enter one headless
  application service. The GUI is an HTTP client. The server has one shared
  selected TV, while every command still supplies an explicit TV ID and
  selection generation.
- 2026-10-15 — Limited listeners to this Mac's loopback interface. The server
  does not start at login; opening the GUI starts it if absent, and the server
  persists when the GUI closes. Users can launch it explicitly for MCP-only
  use. MCP exposes all supported setup and control operations, including an
  explicit select-TV tool and guarded Pairing and forget. Claude Code is the
  first MCP interoperability target.
