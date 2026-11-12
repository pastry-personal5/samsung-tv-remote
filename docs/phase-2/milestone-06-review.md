# P2-M6 requirement review

## Human-run review: 2026-11-12

Reviewed the handoff script, both signed probe builders, the Swift Keychain
probe, and the isolated-account runbook against the current acceptance record.
The review found and corrected these handoff issues:

| Finding | Correction |
| --- | --- |
| Required Keychain setup depended on the rejected, optional LaunchAgent build | Added `--keychain-only`; the required run now stages and installs just the Keychain probe and legacy GUI. Full staging remains available for optional research. |
| The install block set `stage` inside a subshell, so later refresh commands lost the path | Moved the assignment into the parent Terminal and documented resetting it after a new login or Terminal. |
| Bundle-only identity output omitted the separately signed helper; staging HEAD could be mistaken for a binary build revision | Record the helper's code identifier/CDHash separately and label staging HEAD explicitly. A valid GUI signature does not establish that it is a legacy pairing build. |
| A failed verify/delete after replacement could leave an unusable disposable token | Documented stopping that case, forgetting the disposable saved TV and pairing again before the helper's separate mutation case. |

The existing probe checks the exact persisted `dev_<decimal>` account and
isolation variables before accessing Keychain, keeps credential values out
of output, disables interaction, and verifies replacement and final absence.
Automated fixtures cover input rejection and signed handoff preparation;
they do not supply old-GUI Keychain evidence. No human Keychain result or
process-choice acceptance is established by this review. M6 remains Active.

## Historical requirement inventory

Historical review, 2026-10-15. The 2026-11-11
[acceptance and handoff record](milestone-06-acceptance.md) supersedes the gate
assignments below; this table preserves the earlier evidence inventory.

Reviewed 2026-10-15 against that worktree. **M6 remains in progress.**
The candidate contract and probes are reviewable inputs, not an implemented
server or a completed signed migration. This table maps the overview's
completion checklist to evidence and the remaining proof.

| Checklist requirement | Current evidence | Remaining proof |
| --- | --- | --- |
| All TV operations and distinct remote/Wake/readiness outcomes | [Contract](milestone-06-contract.md) route and operation matrices, [API examples](milestone-06-api-examples.md) and [baseline](milestone-06-baseline.md) | Fake-port contract cases for every operation and complete DTO/schema review. |
| Concurrent selection, stale generation, in-flight uncertain write | [Two-client sequence](milestone-06-contract.md#two-client-sequence-to-test) includes expected generation and no replay | Deterministic interleaving test under M7 state owner. |
| Epoch, scoped keys, conflict, retention, crash unknown | [Contract](milestone-06-contract.md#common-envelope-and-admission) specifies 30-minute/32,768-record policy | Capacity/memory, duplicate payload, expiry and restart tests. |
| Atomic subscription and slow observer | [Contract](milestone-06-contract.md#v1-json-and-event-surface) chooses atomic snapshot, shared coarse events for private operations, and 64-event overflow resnapshot | Owner/subscriber race, per-client event filtering and overflow tests. |
| Threat, process shape, signing and client version | [Platform evidence](milestone-06-platform-evidence.md) verifies signed parent-exit, port collision, toy Stop/Resume and forced-crash relaunch; an app-bundled LaunchAgent candidate builds and signs but workspace `status` returns `notFound`; isolated Claude 2.1.300 health check speaks 2026-07-28; macOS 11 floor recorded | Recognized signed LaunchAgent registration/login comparison in an isolated account, old-GUI Keychain, Local Network attribution, real-operation Stop/crash, upgrade/logout/fast-switch tests; signed app client connection. |
| Stable per-user URL and active-client limitation | [Platform candidate](milestone-06-platform-evidence.md#candidate-lifecycle-and-access-decision) specifies manifest, collision failure and future-session reconfiguration; the isolated signed probe rejects an unrelated listener's occupied port | Endpoint implementation, two users and active Claude connection tests. |
| Startup distinct from TV effects | [Contract](milestone-06-contract.md#operation-matrix) and [baseline](milestone-06-baseline.md#behavior-migration) identify GUI-only reconnect | Fake ports show zero TV calls on server start and MCP-only connection. |
| Semantic MCP catalog, examples and budgets | [Corpus](mcp-corpus.json), [budget](milestone-06-mcp-budget.md) and 13-tool catalog; success, stale, uncertain, Pairing and partial forget samples | Real tool schemas, model-visible counts and calls with tool search on/off in signed target. |
| Current-to-target migration and acceptance | [Baseline](milestone-06-baseline.md#behavior-migration) covers coordinator, dispatcher, setup, storage and bundle | Execute each acceptance test as its milestone lands. |
| GUI ownership and complete action sequence | [GUI table and volume sequence](milestone-06-baseline.md#gui-state-ownership) show event, target/epoch/generation/key, receipt and unknown wording | M9 implementation and source boundary check. |
| Server owner, linearization, bounded workers and durable fences | [Internals](control-server-internals.md), [contract limits](milestone-06-contract.md#common-envelope-and-admission), [recovery design](milestone-06-baseline.md#data-recovery-contract-for-m7m9) | M7 fake-port adversarial tests and memory measurement. |
| Saved data, old GUI Keychain and silent migration | [Data inventory](milestone-06-baseline.md#current-data-and-bundle-inventory) records versions/service, backup and test-account isolation; [platform evidence](milestone-06-platform-evidence.md#environment-and-completed-experiment) records a compiled and signed noninteractive probe that has not touched Keychain | Synthetic item made by old signed GUI, both new shapes actually reading/updating/deleting it, locked/denied/upgrade/prompt tests, safe rollback. This is the principal M6 gate. |
| Crash-point recovery across stores | [Fail-closed marker design](milestone-06-baseline.md#data-recovery-contract-for-m7m9) covers first pair, re-pair and forget | Durable marker implementation and crash injection after every store step. |

Apple's [`SMAppService.register()` documentation](https://developer.apple.com/documentation/servicemanagement/smappservice/register%28%29)
confirms that a registered LaunchAgent bootstraps at each later login and
requires macOS 13 or later. This supports the on-demand candidate but does
not satisfy the checklist's signed LaunchAgent experiment.

Review changes made while drafting: same-ID reselect follows the existing
coordinator's generation-changing reconnect, stale Pairing cannot commit after
a concurrent selection change, `cancel_wake` is explicitly in MCP, setup
workflow samples use Pairing outcomes rather than remote-write outcomes,
Pairing observations bind creator/host/generation, and shared coarse events
preserve consecutive revisions without exposing another client's result. The
operation-retention cap was raised while shortening retention so ordinary
remote use does not quickly saturate admission. Lookup after eviction now
reports `outcome_unavailable` rather than pretending it can distinguish an
expired key from one never admitted. The MCP corpus requires a Pairing
generation and uses one-of schemas for probe target and operation lookup;
the model-visible budget was recalculated with those safety fields. None of
these design choices is treated as validated runtime behavior before its
tests pass. Per-device configuration revisions are epoch-scoped because the
current `devices.json` has no revision field; the server epoch invalidates
old revision preconditions after restart without silently rewriting v2 data.
