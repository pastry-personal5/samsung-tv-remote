# Phase 3 decision log

Status: Active

- 2026-11-15 — Owner accepted the P3-M2 gate decisions: configurable tuner,
  GUI-only source editing, and an MCP catalog re-baseline at 58,820 bytes
  (+83). Hardware matrix still not run.

- 2026-11-15 — Owner requested P3-M2 implementation; recorded an offline-code
  exception like M1's. Implemented the five source actions, per-TV source
  configuration with durable `user_confirmation` facts, `SetSources` on the
  shared Wake revision, `source_unconfigured`/`source_hidden` 412 admission
  rejections, device format v3 with `devices.pre-v3.json`, pairing backup v2,
  `PUT /v1/devices/{id}/sources`, MCP `remote_action` names and `inspect_tv`
  sources, and the GUI Sources view and Settings editor; see the
  [implementation record](milestone-02-implementation.md). Defaults taken,
  owner-reviewable: configurable tuner; GUI-only editing. Measured: 1,024-TV
  M2 worst-case snapshot 3,761,227 of 4,194,304 bytes (below M1's 4,071,889
  because source facts drop their session context); v3 device file 2,531,036
  of 4,194,304 bytes; MCP catalog proxy 58,737 → 58,820 bytes (+83, ~21 units),
  not yet offset or re-baselined. Hardware matrix not run.

- 2026-11-15 — Second P3-M2 plan review; see the
  [plan review](milestone-02-plan-review.md#second-review--2026-11-15).
  Hidden/unconfigured source keys are now 412 admission rejections, not
  `not_sent` results, with fixed precedence. Named the wildcard command arms
  that would skip the stale-selection fence, limited the session fence to
  Select, kept the frozen diagnostic mapping unchanged behind an agreement
  test, pinned PascalCase action names, and made pairing backups version 2
  unconditionally. The tuner reading moved to the owner gate.

- 2026-11-14 — Reviewed and strengthened P3-M2 against the current M1 and
  runtime/storage/client seams; see the [plan review](milestone-02-plan-review.md).
  Specified durable-fact decoder exceptions and persisted confirmation times,
  typed uncertain-write reconciliation, receipt-preserving deduplication,
  versioned pairing backups and recoverable migration on every write path.
  Clarified omitted source details, full replacement validation, distinct
  hidden/unconfigured reasons, label identity and hardware acceptance. Added
  two-client, failure, disk and framed-size proof obligations. Implementation,
  MCP policy/budgets and hardware acceptance remain open.

- 2026-11-14 — Implemented the explicitly gated `p3-spike` owner/controller
  under the [frozen private contract](milestone-01-diagnostic-contract.md).
  Added one cancelable lease, finite intents/alias guards, timeout quarantine,
  bounded diagnostic codecs and sanitized draft exports. Prepared a preserved
  Apple-signed candidate and verified it without requesting Keychain access.
  Operator confirmed a hands-on session is available; owner replacement and
  individual TV effects still require their own intent. No capture is invented.

- 2026-11-14 — Planned P3-M2 in its [overview](milestone-02-overview.md) and
  [architecture](milestone-02-architecture.md). Source keys become five
  `RemoteAction` values on the existing admission path. Per-TV Shown/Hidden
  configuration with optional labels is stored in the saved-TV record (device
  format v3, fail-closed downgrade, one-time backup) and shares the Wake config
  revision. Shown keys project as durable `user_confirmation` facts with no
  session context. Hidden is a preference, not `unsupported`. The tuner is
  configured like an HDMI port; the Sources menu is always offered. Labels stay
  out of non-selected snapshot entries to fit the 1,024-TV size limit. Open:
  implementation authorization, MCP label editing (GUI-only recommended), and
  the MCP budget lane for the enum and `inspect_tv` growth.

- 2026-11-14 — Owner requested P3-M1 implementation. Recorded an offline-code
  exception to predecessor ordering, without accepting Phase 2, hardware or
  MCP budgets. Implemented bounded semantic evidence, session fencing,
  content-free IME seams and shared HTTP/SSE/native/MCP inspection projections;
  see the [implementation record](milestone-01-implementation.md). The signed
  diagnostic controller, reviewed captures and production parsers remain open.
  The phase and M1 are Active, not Done.

- 2026-11-14 — Reviewed P3-M1 against the actual session and action seams; see
  the [plan review](milestone-01-plan-review.md). Added the missing diagnostic
  preparation slice and [physical-TV automation contract](milestone-01-spike-automation.md),
  plus a tested offline matrix recorder. Made uncorrelated reply timeout
  quarantine, fresh app aliases after reconnect, operation-specific acquisition
  gating, atomic invalidation and ID integrity explicit. Live harness and
  hardware evidence remain pending; no predecessor waiver or physical effect
  is claimed.

- 2026-11-14 — Planned P3-M1 in its [overview](milestone-01-overview.md)
  and [architecture](milestone-01-architecture.md). Defined the target-TV
  spike, evidence/freshness model, generation fences, passive M1 acquisition,
  existing HTTP/SSE and MCP read projections, and deterministic acceptance
  matrix. M1 adds no persistent catalog or feature controls. Phase 2 completion,
  the MCP budget decision, and target-TV evidence remain open gates; planning
  does not waive them or claim the spike has run.

- 2026-11-14 — Revised the plan. Split capability evidence from request
  outcomes and reused Phase 2's `not_sent`/`written`/`uncertain` set. Made
  source ports user-configured, because no probe can prove a source key.
  Added a target-TV frame spike to P3-M1, no-replay and untrusted-TV-data
  rules, server-owned IME sessions, per-TV storage covered by Forget
  recovery, config revisions on setting changes, and the MCP budget's lack
  of headroom. Recorded open decisions, including MCP text input and
  text-safe request fingerprints.

- 2026-11-14 — Planned Phase 3 from the archived Phase 2 proposal. Old P2-M1
  to P2-M5 stay Dropped; their scope returns as P3-M1 to P3-M5, adapted so the
  Control API Server owns probes, caches, and requests for both GUI and MCP.
  The phase starts after Phase 2 is Done.
