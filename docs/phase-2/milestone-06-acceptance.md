# P2-M6 acceptance and handoff

Status: Active. Reviewed 2026-11-11; M6 is not complete.

M7 completed on 2026-11-12 under its authorized entry exception. Its
[completion review](milestone-07-review.md) supplies the assigned deterministic
runtime/GUI evidence and measured limits. Subsequent owner decisions select
same-executable server mode and remove legacy device-key migration from scope;
see the dated updates below. M6 is not automatically marked complete.

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

## Owner process selection: 2026-11-12

Use an on-demand background process running the same signed executable in
`--server` mode. No login registration or root daemon is selected. GUI and
server are separate processes with one shared code identity; this is not an
OS-enforced credential boundary between modes. Keep separate composition
modules and prevent the GUI mode from owning TV/storage after M9 cutover.

Signed operation still needs verification under the selected identity; process
selection alone does not prove credential access. The later scope decision
below supersedes the legacy migration experiment.

## Owner credential scope update: 2026-11-12

The owner confirmed that no device-key migration, including synthetic legacy
migration, is needed. Remove the old-GUI-to-server credential migration and
synthetic migration experiment from required acceptance. Do not build migration
machinery, delete existing items or transfer credentials as part of this scope
change. Historical probe instructions remain reference material, not a gate.

Keep Keychain storage for newly created pairing credentials. M9 must verify
signed server save/read/update/delete, persistence across restart and upgrade,
zero routine prompts, and noninteractive locked/denied/interaction-required
failures. Development uses fake credential ports. This changes the acceptance
scope; it does not claim those signed tests passed or mark M6 Done.

## Owner initial-profile decision: 2026-11-12

Start the new server with a fresh, empty device profile. Initial state has no
saved TVs, selected TV, trust records or pairing credentials; adding a TV and
Pairing are explicit first-use operations. Do not import legacy device data.
Use a distinct profile/storage identity so old preferences, trust, recovery
markers and Keychain items cannot be loaded accidentally. Missing credentials
in this new profile are expected; locked/denied access is still an error.

This decision authorizes the fresh-start implementation, not deletion of
existing local files or Keychain items during planning. Legacy cleanup is a
separate action. Restart of an initialized new profile restores its saved TVs;
fresh start is not a reset on every launch. M9 must prove empty first launch,
new pairing persistence, restart/upgrade continuity and single profile ownership.

## M6 exit and evidence

| Gate | Evidence needed for M6 | Current state |
| --- | --- | --- |
| Operation and ownership contract | Complete operation/error/event matrices, migration map, admission and recovery rules, and assigned downstream tests | Documented; admission and timing ambiguities corrected in this review |
| Lifecycle feasibility | Focused signed detached-launch, parent-exit, occupied-port, Stop/Resume and crash probes with isolated storage | Re-run and passed on 2026-11-11; accepted for M6 design freeze |
| Endpoint and MCP feasibility | Stable per-user URL/credential-helper design and versioned fake-endpoint negotiation; explain active-client limitation | CLI 2.1.322 fake negotiation passed on 2026-11-11; accepted for M6 design freeze; real endpoint is M8/M10 work |
| Legacy Keychain migration | No migration required by owner decision on 2026-11-12 | Removed from scope, including the synthetic old-GUI experiment; fresh-credential signed behavior is M9 evidence |
| Process choice | Document chosen identity, restrictions and M9 verification | Owner selected on-demand same-executable `--server` mode on 2026-11-12; signed Keychain compatibility and M9 final-identity verification remain open |
| Measurement | Reproducible sanitized catalog/workflow proxy and explicit real-client measurement owner | Proxy reproduced on 2026-11-11; actual tokens and model turns belong to M10 |
| Acceptance | Update overview, architecture, evidence and changelog; record owner acceptance and required Cargo checks | Cargo checks and 141 tests passed on 2026-11-11; final acceptance review remains pending under the revised scope |

Each new experiment records date, source revision, OS and client versions,
bundle/code identity, isolated profile, exact command, expected result,
actual result, prompt count, and pass/fail/deferred with reason. Store no
credentials, addresses, personal account names or raw network captures.
Rebuild missing temporary probes; do not infer that an old handoff directory,
test account, or signed binary still exists. The dated platform evidence is
historical evidence, not a statement about the current machine's state.

A feasibility failure may change the process design. A probe with a different
identity may support a conditional choice but cannot waive M9's real-identity
cutover gate. The removed legacy migration experiment is no longer an M6
blocker; fresh-credential production tests are assigned below.
LaunchAgent registration is optional research on a rejected alternative.

## Handoff and implementation tests

| Owner | Required implementation evidence |
| --- | --- |
| M7 | Fake-port operation parity, atomic admission and deduplication, two-client selection interleavings, late effects, durable-store crash injection, bounded memory and fake-time deadline/cancellation tests |
| M8 | Authenticated HTTP/readiness/events, Host/Origin and input validation, bootstrap peer checks, rotation/revocation, single-instance locking, endpoint collisions, safe manifest/socket paths, and isolated development profile |
| M9 | Full GUI API parity, signed chosen process identity, fresh-credential Keychain lifecycle and profile cutover backup/rollback, Local Network attribution/denial/recovery, zero routine prompts and atomic profile cutover |
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
