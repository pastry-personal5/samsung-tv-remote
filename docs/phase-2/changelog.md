# Phase 2 decision log

Status: Active

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
