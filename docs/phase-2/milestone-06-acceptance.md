# P2-M6 acceptance and handoff

Status: Active. Reviewed 2026-11-11; M6 is not complete.

M7 completed on 2026-11-12 under its authorized entry exception. Its
[completion review](milestone-07-review.md) supplies the assigned deterministic
runtime/GUI evidence and measured limits. This does not accept the Keychain
experiment or choose the final background process; those M6 gates remain open.

## Owner decisions

The owner confirmed these choices during the 2026-11-11 review:

- M6 accepts the design and focused platform probes. Production server, HTTP,
  GUI and MCP tests belong to M7–M11; they do not depend on existing in M6.
- Target the owner's Mac and a local signed installation. No notarization or
  public distribution acceptance is required in Phase 2. Record the actual
  macOS version and signing identity with each platform test. The old binary's
  macOS 11 deployment metadata is not a tested support promise.
- Start on demand, remain available after GUI close, never start at login,
  and honor sticky Stop until explicit Resume in the GUI. MCP cannot Resume.
- One explicit migration/Keychain approval is acceptable. Subsequent routine
  use must have zero prompts. Denied, locked or interaction-required access
  blocks visibly without background prompting or automatic retry loops.
- MCP Pair and Forget use explicit typed confirmation steps; the client owns
  human approval. The server validates target, observation and preconditions,
  but does not claim those fields prove human approval. Pairing still requires
  TV approval. No extra GUI approval ceremony is required for MCP.

## M6 exit and evidence

| Gate | Evidence needed for M6 | Current state |
| --- | --- | --- |
| Operation and ownership contract | Complete operation/error/event matrices, migration map, admission and recovery rules, and assigned downstream tests | Documented; admission and timing ambiguities corrected in this review |
| Lifecycle feasibility | Focused signed detached-launch, parent-exit, occupied-port, Stop/Resume and crash probes with isolated storage | Re-run and passed on 2026-11-11; accepted for M6 design freeze |
| Endpoint and MCP feasibility | Stable per-user URL/credential-helper design and versioned fake-endpoint negotiation; explain active-client limitation | CLI 2.1.322 fake negotiation passed on 2026-11-11; accepted for M6 design freeze; real endpoint is M8/M10 work |
| Keychain feasibility | Synthetic old-GUI item in isolated desktop account; signed candidate read/update/delete and denied/locked/prompt results | Not run; prepared probe identities do not prove final server identity |
| Process choice | Select same-executable server mode or signed helper from evidence; document restrictions and M9 verification | Conditional detached-process candidate; final identity/access decision open |
| Measurement | Reproducible sanitized catalog/workflow proxy and explicit real-client measurement owner | Proxy reproduced on 2026-11-11; actual tokens and model turns belong to M10 |
| Acceptance | Update overview, architecture, evidence and changelog; record owner acceptance and required Cargo checks | Cargo checks and 141 tests passed on 2026-11-11; final acceptance waits for the Keychain gate |

Each new experiment records date, source revision, OS and client versions,
bundle/code identity, isolated profile, exact command, expected result,
actual result, prompt count, and pass/fail/deferred with reason. Store no
credentials, addresses, personal account names or raw network captures.
Rebuild missing temporary probes; do not infer that an old handoff directory,
test account, or signed binary still exists. The dated platform evidence is
historical evidence, not a statement about the current machine's state.

A feasibility failure may change the process design. A probe with a different
identity may support a conditional choice but cannot waive M9's real-identity
cutover gate. A missing focused Keychain experiment remains an M6 blocker;
production-only tests are assigned below instead of blocking design work.
LaunchAgent registration is optional research on a rejected alternative.

## Handoff and implementation tests

| Owner | Required implementation evidence |
| --- | --- |
| M7 | Fake-port operation parity, atomic admission and deduplication, two-client selection interleavings, late effects, durable-store crash injection, bounded memory and fake-time deadline/cancellation tests |
| M8 | Authenticated HTTP/readiness/events, Host/Origin and input validation, bootstrap peer checks, rotation/revocation, single-instance locking, endpoint collisions, safe manifest/socket paths, and isolated development profile |
| M9 | Full GUI API parity, signed chosen process identity, actual legacy-item migration and backup/rollback, Local Network attribution/denial/recovery, zero routine prompts and atomic profile cutover |
| M10 | Real signed `/mcp` with configured Claude Code, protocol conformance, tool confirmation, absent-server start/sticky Stop, auth refresh, actual model-visible token and call budgets with tool search on/off |
| M11 | Upgrade/logout/sleep/wake/fast-user-switch integration, two-client failure/restart review, accessibility, sanitized hardware matrix and final legacy-path removal |

M6 freezes the proposed interface and invariants, not an unchangeable wire
format. Before M8 publishes it, M7 records measured capacity/timing changes
in the contract, updates examples/corpus and checks all consumers. A semantic
change after publication requires an explicit API/protocol compatibility
choice rather than a silent adapter-specific exception.

## Review fixes and remaining implementation details

- Admission is observable: a retained policy failure includes operation
  metadata even when HTTP reports an error. Malformed/authentication/epoch/
  ingress-capacity failures are not new operations. Known duplicates still
  resolve when capacity is exhausted. No rejected request begins a TV effect.
- Outer supervision deadlines do not silently lengthen existing stage waits.
  Keep current waits during extraction; M7 must justify and test changes.
- Bound more than worker count: M7 must budget candidates, observations,
  subscriber count, response sizes, pending operations and per-client usage.
  Expiry is monotonic within an epoch; wall-clock timestamps are display data.
  Pending work must terminalize on deadline or supervisor failure, so it cannot
  occupy registry capacity forever. M8 adds bounded connection/idle timeouts.
- A Wake operation captures the validated active interface/configuration at
  admission and uses that immutable configuration for its single send. Later
  edits apply to new operations. Selection/forget/Stop fences still prevent a
  pending send. M7 tests an edit racing the send; no late packet is replayed.
- Same-ID selection still means generation-changing reconnect. GUI attach
  must not overwrite a concurrent client's selection. GUI launch reconnect
  must revalidate the attached epoch/target/generation before requesting it.
- Registry capacity, event overflow and credential rotation need independent
  tests: retention is scoped to stable authenticated identity, not the current
  bearer string. A refreshed credential must not turn an old key into a new
  side effect. Revoke authorization before any new effect admission.

## References checked during review

- [MCP 2026-07-28 transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports)
  defines per-request metadata and request-scoped HTTP responses.
- [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp)
  documents dynamic header helpers. It does not establish compatibility with
  the future signed product server; M10 must demonstrate that behavior.
- [Overview](milestone-06-overview.md), [contract](milestone-06-contract.md),
  [platform evidence](milestone-06-platform-evidence.md), and
  [isolated-account runbook](milestone-06-human-runbook.md).
