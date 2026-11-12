# P2-M9 acceptance audit

Status: Incomplete. This maps the current [completion checklist](milestone-09-overview.md#completion-checklist)
to evidence and remaining work; it is not a completion review or release approval.
Code/configuration checked: `0a396a6`; current architecture/evidence: `123f720`.
The delegated final run passed 323 package tests across 24 suites plus two
explicit helper tests, formatting and all-target Clippy. Full output is retained
privately as `/private/tmp/samsung-m9-cargo-test-0a396a6.log`.

Checklist numbers below follow the overview's bullet order. A passing automated
test establishes only its stated scope. Native results and their limitations
are recorded in the [implementation ledger](milestone-09-implementation.md).

| Item | Authoritative evidence inspected | Remaining acceptance work |
| --- | --- | --- |
| 1. Accepted predecessor and signed gates | M8 completion/wire/bootstrap references are in the ledger. Native same-executable/wrong-peer checks pass. The local self-signed changed build is denied Keychain access despite equal designated requirements. | M6/M9 final signing and credential gates remain open. Apple disposable-item success does not prove existing live-role access. |
| 2. All GUI operations use the API | Default composition and presentation use the typed client. `client_port_captures_remote_wake_cancel_connect_and_selection_preconditions` checks five captured requests, unique keys and epoch/target/generation facts. `client_workflow_probe_pair_and_repair_require_the_current_explicit_confirmation`, discovery/recovery cases and HTTP setup tests cover setup flows. | Complete signed native GUI parity review, including fixture Pairing/Wake and restore. |
| 3. Ordering, tracking, private history and idle launch | `client_workflow_select_save_connect_uses_terminal_preconditions_in_both_event_orders` asserts both receipt/event orderings and the guarded continuation. Tracker tests cover lost admission, monotonic settlement, cancellation, changed epochs, private-reference saturation and wrong lookup identity. Projection tests bound and kind-check private references. HTTP `idle_launch_connect_*` tests check competing ready/busy clients and retained success. | Automated coverage is present. Retain it in final validation; native review must not substitute attachment for TV readiness. |
| 4. External selection and dirty drafts | `client_workflow_external_selection_or_config_change_never_retargets_a_dirty_draft`, pending-save/new-edit, same-device generation/configuration, expired-probe and epoch-change cases check the captured context. Window/navigation tests preserve local state. | Automated coverage is present; include draft/conflict behavior in the signed visual review. |
| 5. Outage, epochs, backoff and no replay | `tests/control_client.rs` checks immediate attempt, repeated one-hour cap, healthy reset, sleep, blocked states and explicit recheck. `gui_actor_rotates_to_a_fresh_snapshot_then_blocks_on_revocation_without_replaying_any_tv_effect` exercises real client/HTTP/bootstrap composition with fake credential/TV ports. `gui_client_reattaches_after_process_outage_without_replaying_setup_connect_or_action` crosses a subprocess outage. | Observe production close/reopen and recovery in a visible signed GUI. Fake credential ports do not prove native Keychain recovery. |
| 6. Lifecycle and distinct blocked states | `client_lifecycle_*` checks shared-impact Stop, epoch fencing, durability versus settlement, fresh-state barriers, explicit recheck and detach. Two-client process tests exercise sticky Stop/Resume. Native original-signer CLI Stop/Resume passed. Local Network failures have typed HTTP/UI mappings. | Signed visual review of blocked states and controlled-LAN permission denial/recovery remain open. A mapped error is not evidence of actual OS attribution or permission behavior. |
| 7. Bootstrap identity and credential secrecy | `SignedPeer` checks the current certificate-backed requirement and connected kernel audit token. Native wrong peer receives denial before challenge/credential disclosure. Bootstrap tests cover one-use listener proof, replay, context binding, expiry, rotation and revocation. | Reverify final selected signing identity and live-profile authentication after the approved access handoff. Same executable does not isolate modes or prove human intent. |
| 8. Signed bundle and credential lifecycle | Original signed empty-profile launch with populated legacy data, preserved hashes, detached owner and native CLI lifecycle pass. The constrained probe performs synthetic CRUD in the actual fresh services. Apple changed-build disposable CRUD passes; local self-signed cross-build access fails. | Existing API role access under the selected final signer, fixture-backed first Pairing/Wake, saved-profile restore, production GUI close/reopen, native blocked/locked/interaction-required behavior and controlled-LAN review are unproven. No credential reset or namespace replacement can satisfy these gates. |
| 9. Deterministic flow and two-client evidence | Fake Iced tasks, HTTP setup/error/cancellation matrices, two-client selection/idle races and subprocess outcome recovery are covered by the passing suite. The screenshot and CUA lifecycle review in the ledger are explicitly development/fake evidence. | Production native screenshots and sanitized observations remain missing. Real-TV smoke execution is supplementary. |
| 10. GUI boundary and namespace isolation | `in_process` is removed. Required `tests/gui_boundary.rs` invokes the checker and five-case negative corpus. Production mode owns adapters; default composition fails unsigned initialization before opening the profile. Namespace tests reject cross-profile endpoint/Stop/bootstrap/development records. | Automated boundary is established; rerun the required checks after any further source changes. |
| 11. Atomic cutover and prior-owner exclusion | The source cutover removed all GUI runtime paths together. Before the first production open, the native preflight found no prior GUI/server owner. New storage remained empty and legacy hashes stayed unchanged. Failed candidate restart was rolled back to the unchanged original owner without deleting data. | Failed signed gates still block release acceptance. Do not ship a mixed-mode fallback or claim the experimental live owner is a completed release. |
| 12. Validation, current docs and final review | Required checks pass on committed source. `123f720` updates architecture/UX and records the implementation/native findings. The tested contribution-guide additions remain in the working tree with existing planning edits. | Finish native gates, update tested final launch/signing instructions, rerun appropriate final checks and write the requirement-by-requirement completion review with M11 handoff. |

## Current native handoff

The standalone helper at `target/m9-access-handoff.A95sIg/authorize_api_access`
has passed inspection against the Apple Development candidate. Its private
manifest records matching strict signatures/designated requirements, public
hashes, revision `0a396a6`, no Keychain requests and unchanged credential values.
Request mode has not run. The owner has been asked to approve the selected
Apple signer and native access to the existing fresh-service `gui`/`mcp` items.
This grants no access to legacy or TV credentials and changes no credential
values. Routine application queries remain UI-off.

The desktop inventory still reports Samsung TV Remote running, but the latest
exact-path window inspection returns `cgWindowNotFound`. This observation
failure does not establish that the process exited or justify restarting it.
No production visual acceptance is inferred from process liveness or the fake
renderer capture. The owner has also been asked whether the window is visible.

Read-only native revalidation confirmed the original GUI and server at their
expected executable path, `serving/ready` status, unchanged original/helper/
candidate hashes and unchanged legacy hashes. No request mode, role read,
restart or permission action occurred.

The audit also found that existing tooling had no signed real-adapter
synthetic Pairing/Wake workflow. The new [TLS fixture runbook](milestone-09-native-fixture.md)
addresses that preparation gap without requiring physical TV hardware. Its
loopback self-checks do not establish production or Local Network behavior.
A separate controlled LAN host is still needed for the OS permission gate;
none has been identified or contacted. No app address policy is relaxed to
let loopback testing count as that gate.
