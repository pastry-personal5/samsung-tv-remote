# P2-M11 acceptance matrix

Status: Active. This matrix defines requirements; the
[implementation ledger](milestone-11-implementation.md) maps named tests and
the [review](milestone-11-review.md) records execution status. The
[native runbook](milestone-11-native-runbook.md) is prepared, not executed.

The [overview](milestone-11-overview.md) owns completion; the
[architecture](milestone-11-architecture.md) owns implementation contracts.
At slice 1, expand each row into named existing or new tests and native steps.
Use `not run`, `passed`, `failed`, `blocked` or `waived` for individual
evidence records. Record the reason and next action for every failed or blocked
case. `waived` applies only to rows named in the
[owner decision](phase-2.md#owner-decisions). Such a record names its
substitute evidence and residual risk, and it is never counted as passed.

## Evidence rules

Each execution record identifies scenario, source revision, artifact hash,
signing identity class/TeamIdentifier, macOS and client/runtime versions,
fixture namespace/topology, expected result, actual result and sanitized
artifact links. Include relevant test names and process/epoch/revision facts.
Never include bearer values, TV addresses, private input or owner profile copies.
Native runbooks must specify setup, action, observable pass condition and
cleanup; prepare them before touching a signed test installation.

Reuse earlier evidence only when its source/artifact, topology and contract
match the requirement. Mark earlier results as historical where they do not.
A real adapter talking to a same-Mac fixture is useful protocol evidence, but
does not establish separate-host permission behavior or physical TV response.
A client configuration entry or successful endpoint probe alone does not prove
that Claude Code attached or that a model saw the intended tool definitions.

## Deterministic integration

| ID | Scenario and required result | Test owner/seam |
| --- | --- | --- |
| D1 | GUI and MCP share one selected target/runtime; concurrent selection and config changes reject captured stale intent without retargeting. Compare facts at matching epoch/revision. | Combined transport fixture; client projections and Iced fake-client tasks. |
| D2 | Completion races atomic subscription; slow/full/closed SSE stream resnapshots with bounded recovery and does not stall effects. | Shared HTTP/SSE fixture, client tracker and runtime observer tests. |
| D3 | Reply loss before/after admission and terminalization recovers by original key/handle. Same principal/key and equivalent typed command deduplicate across routes; changed payload conflicts; another principal cannot read the result. | Shared HTTP/MCP fixture, operation registry and private lookup tests. |
| D4 | Expiry and crash change lookup to unknown; changed epoch rejects old handles/mutations. Neither client replays a TV write. | Fake clock, effect counters and isolated server-process fixture. |
| D5 | Mixed mutations, POST reads, waiters, SSE and stalled/malformed bodies obey shared/per-role capacity; recovery/management progresses and cancellation releases reservations. | HTTP transport, MCP wait and runtime-limit fixtures. |
| D6 | Pairing/re-pair, cancellation, stale certificate confirmation, Forget and concurrent cleanup preserve single policy and durable markers under credential/journal/configuration partial failure. | Existing pairing/Forget/storage fault ports plus combined client fixture. |
| D7 | GUI exits while MCP is active; owner crash/restart reconnects GUI from a fresh snapshot and preserves sticky Stop when present. | Process fixture with typed GUI client and MCP requests. |
| D8 | Concurrent helper start and final enrollment-guard release have diagnosed behavior, no duplicate owner and no prematurely released lock. Delayed helpers cannot use a superseded grant. | M10 helper/enrollment regression tests with controlled interleavings. |
| D9 | Removal sequence (Disconnect, then Stop) with running and already stopped owners. It races Connect/helper startup, crashes at each durable step, survives reply loss and retries safely. A leftover entry plus a relocated or reinstalled bundle reaches access only via explicit repair/Connect. Changed entries remain intact; progress/unknown states are visible. | Existing enrollment offline/durable-disable, connection conflict and relocation tests; add only missing cases. |
| D10 | Installer stage/verify/settle/publish/rollback handles PID reuse, concurrent launch/installer, timeout, signature/rename failure and rollback failure without losing the previous bundle or data. | Installer Python fakes and Cargo bundle suites. |
| D11 | Legacy/fresh namespace isolation, retained record IDs/trust/credentials and stopped/disabled state survive upgrade, the removal sequence and rollback. Any schema conversion has backup/restore fault coverage. | Namespace, repository, credential and bundle fixtures. |
| D12 | GUI cannot import runtime/network/storage adapters or carry raw policy/session objects; fake-client projection and captured-intent tests enforce the boundary. | Mandatory boundary checker/self-test and Iced client tests. |

## Signed macOS, installed client and target TV

| ID | Topology and scenario | Required observation |
| --- | --- | --- |
| N1 | Final signed bundle; cold and simultaneous GUI/helper launch, compatible attach, unrelated listener and hostile/wrong-signature peer. | Exactly one authenticated owner; fail closed for unrelated peers; no manual port/token step or routine prompt. |
| N2 | Installed GUI plus configured Claude Code; GUI close/reopen, explicit Stop/Resume, incompatible restart and crash recovery. | MCP survives GUI exit; both clients honor Stop; fresh epoch/snapshot after restart; uncertain outcomes stay unknown with no replay. |
| N3 | Same installation before/after sleep, with an in-flight action and a broken event stream. | Bounded reconnect/resnapshot; no spontaneous Resume, selection retarget or duplicate write. |
| N4 | Signed older/new candidate install and retained-bundle rollback, with active clients, detached server and stopped profile variants. | Old owner settles before replacement; signing and data remain valid; API/TV items work UI-off; incomplete actions remain honest. |
| N5 | **Waived.** Two disposable macOS user accounts and fast user switching. | Substitute: deterministic cross-UID peer/owner rejection and per-user runtime-directory tests. Residual risk: real session switching is not observed. |
| N6 | Isolated signed credentials: normal, denied, locked and interaction-required, including upgrade/reconnect. | Zero routine prompts; explicit blocked status, no unattended dialog/retry loop; consent/unlock is recorded separately. |
| N7 | **Waived.** Controlled separate-host LAN fixture with server-attributed permission, allow, deny/revoke and restore. | Substitute: same-Mac fixture protocol evidence and N10's observed TV traffic. Neither proves attribution. Residual risk: native denial/revoke recovery is not observed. |
| N8 | Installed client and signed isolated corpus harness; all nine M6 workflows, 15-tool parity, search on/off, all-tools-upfront fallback, slow lookup and auth refresh. | Actual visible definitions/results/calls/turns meet budgets; guard/confirmation/unknown facts retained. Disposable-account production helper smoke is **waived**. Substitute: this harness with an isolated `CLAUDE_CONFIG_DIR` plus development-namespace helper tests. Residual risk: the production identity/Keychain path is unobserved in a fresh account. |
| N9 | Final signed GUI; keyboard-only now, VoiceOver `blocked` until scheduled, across Power/Remote/Settings, setup, selection, servers/MCP, confirmation and error/recovery flows. | Reachable controls, meaningful names/roles/state, visible focus, usable traversal/escape and restored focus; status/outcome changes announced appropriately. Record defects and recheck fixes. |
| N10 | Physical target TV; discovery/manual probe, fingerprint confirmation, Pair/re-pair, selection, connect, representative remote keys, network loss, Wake/cancel and Forget. | Correct guarded target and cleanup; one send per intent; observed screen/player/power response recorded separately from write/packet/readiness. Unsupported or unobserved behavior is not claimed. |
| N11 | The owner's signed installation, run hands-on only with the owner's explicit consent at that session (signed bundles always use the production profile). Prepare a runbook first, ending in recovery: Resume, Connect, Verify. Run the documented Disconnect → Stop → Trash sequence with a running and a stopped owner, a partial config conflict, then reinstall. | Durable MCP disable and Stop, settled owner, no helper resurrection, retained TV data; conflict is recoverable. Reinstall alone does not restore access. |

Only N10 needs the physical TV. Native account, permission and credential
fixtures require their own available topology; do not test destructive cases
against the owner's active installation. If an account, separate host, installed
client measurement facility or TV is unavailable, record `blocked` and retain
the corresponding completion gate, unless the owner decision waives that row. Planning does not authorize credential
reset, whole-keychain locking or owner-data deletion.

## Final review

- Link D1–D12 and N1–N11 to actual evidence in the implementation ledger and
  final requirement-by-requirement review. A requirement may need multiple rows.
- Record final formatting, all-target Clippy and full Cargo test outcomes from
  delegated validation. Investigate failures; retain prior intermittent results
  even when later runs pass.
- Review sanitized artifacts for secrets and network details, update current
  architecture/UX/user/troubleshooting guidance and the phase decision log.
- Mark the milestone Done only when every overview checkbox has accepted
  evidence, or its row is waived by the recorded owner decision. Any other
  deferral documents what remains; it does not satisfy the gate.
