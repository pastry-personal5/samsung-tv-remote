# Phase 2 decision log

Status: Active

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
