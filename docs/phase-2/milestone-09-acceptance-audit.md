# P2-M9 acceptance audit

Status: Incomplete. This maps the current [completion checklist](milestone-09-overview.md#completion-checklist)
to evidence and remaining work; it is not a completion review or release approval.
Baseline: `a17c295`; follow-up working-tree changes add explicit application
API access. Existing Makefile and M10 planning edits are preserved. Fresh
delegated validation passes 328 package tests across 23 nonempty suites,
formatting, all-target Clippy, example compilation/tests and whitespace checks.
The two reader tests now belong to the library suite. Package output is retained
at `/private/tmp/samsung-m9-api-access-cargo-test.log`. The unchanged fixture
previously passed eight tests. These results do not establish native acceptance.

Checklist numbers below follow the overview's bullet order. A passing automated
test establishes only its stated scope. Native results and their limitations
are recorded in the [implementation ledger](milestone-09-implementation.md).

| Item | Authoritative evidence inspected | Remaining acceptance work |
| --- | --- | --- |
| 1. Accepted predecessor and signed gates | M8 completion/wire/bootstrap references are in the ledger. Native same-executable/wrong-peer checks pass. The local self-signed changed build is denied Keychain access despite equal designated requirements. | M6/M9 final signing and credential gates remain open. The actual Apple application now passes existing-role access and authenticated restart after explicit app-path approval. Final bundle and remaining native gates still apply. |
| 2. All GUI operations use the API | Default composition and presentation use the typed client. `client_port_captures_remote_wake_cancel_connect_and_selection_preconditions` checks five captured requests, unique keys and epoch/target/generation facts. `client_workflow_probe_pair_and_repair_require_the_current_explicit_confirmation`, discovery/recovery cases and HTTP setup tests cover setup flows. | Signed same-Mac fixture Check/Pair/re-pair, one remote action and saved-profile restore pass. Wake and the remaining visual parity review stay open. |
| 3. Ordering, tracking, private history and idle launch | `client_workflow_select_save_connect_uses_terminal_preconditions_in_both_event_orders` asserts both receipt/event orderings and the guarded continuation. Tracker tests cover lost admission, monotonic settlement, cancellation, changed epochs, private-reference saturation and wrong lookup identity. Projection tests bound and kind-check private references. HTTP `idle_launch_connect_*` tests check competing ready/busy clients and retained success. | Automated coverage is present. Retain it in final validation; native review must not substitute attachment for TV readiness. |
| 4. External selection and dirty drafts | `client_workflow_external_selection_or_config_change_never_retargets_a_dirty_draft`, pending-save/new-edit, same-device generation/configuration, expired-probe and epoch-change cases check the captured context. Window/navigation tests preserve local state. | Automated coverage is present; include draft/conflict behavior in the signed visual review. |
| 5. Outage, epochs, backoff and no replay | `tests/control_client.rs` checks immediate attempt, repeated one-hour cap, healthy reset, sleep, blocked states and explicit recheck. `gui_actor_rotates_to_a_fresh_snapshot_then_blocks_on_revocation_without_replaying_any_tv_effect` exercises real client/HTTP/bootstrap composition with fake credential/TV ports. `gui_client_reattaches_after_process_outage_without_replaying_setup_connect_or_action` crosses a subprocess outage. | Signed GUI exit leaves the same serving epoch; reopen visibly reconnects. Explicit app-path credential approval recovers native routine access. Other blocked-store cases remain open. |
| 6. Lifecycle and distinct blocked states | `client_lifecycle_*` checks shared-impact Stop, epoch fencing, durability versus settlement, fresh-state barriers, explicit recheck and detach. Two-client process tests exercise sticky Stop/Resume. Native original-signer CLI Stop/Resume passed. Local Network failures have typed HTTP/UI mappings. | Signed visual review of blocked states and controlled-LAN permission denial/recovery remain open. A mapped error is not evidence of actual OS attribution or permission behavior. |
| 7. Bootstrap identity and credential secrecy | `SignedPeer` checks the current certificate-backed requirement and connected kernel audit token. Native wrong peer receives denial before challenge/credential disclosure. Bootstrap tests cover one-use listener proof, replay, context binding, expiry, rotation and revocation. | Apple candidate signature and live-profile authentication pass after actual-app approval. Final selected build/wrong-peer review remains separate. Same executable does not isolate modes or prove human intent. |
| 8. Signed bundle and credential lifecycle | Original signed empty-profile launch with populated legacy data, preserved hashes, detached owner and native CLI lifecycle pass. The constrained probe performs synthetic CRUD in the actual fresh services. Apple changed-build disposable CRUD passes; local self-signed cross-build access fails. | Existing API role access, same-Mac fixture first Pairing/re-pair, saved-profile restore and production GUI exit/reopen pass. Wake, full native blocked/locked/interaction-required behavior and controlled-LAN review remain open. No credential reset or namespace replacement can satisfy these gates. |
| 9. Deterministic flow and two-client evidence | Fake Iced tasks, HTTP setup/error/cancellation matrices, two-client selection/idle races and subprocess outcome recovery are covered by the passing suite. The screenshot and CUA lifecycle review in the ledger are explicitly development/fake evidence. | Production blocked, connected-empty and restored-fixture screenshots are recorded below; remaining native parity is still open. Real-TV smoke execution is supplementary. |
| 10. GUI boundary and namespace isolation | `in_process` is removed. Required `tests/gui_boundary.rs` invokes the checker and five-case negative corpus. Production mode owns adapters; default composition fails unsigned initialization before opening the profile. Namespace tests reject cross-profile endpoint/Stop/bootstrap/development records. | Automated boundary is established; rerun the required checks after any further source changes. |
| 11. Atomic cutover and prior-owner exclusion | The source cutover removed all GUI runtime paths together. Before the first production open, the native preflight found no prior GUI/server owner. New storage remained empty and legacy hashes stayed unchanged. Failed candidate restart was rolled back to the unchanged original owner without deleting data. | Failed signed gates still block release acceptance. Do not ship a mixed-mode fallback or claim the experimental live owner is a completed release. |
| 12. Validation, current docs and final review | Required checks pass on committed source. `123f720` updates architecture/UX and records the implementation/native findings. The tested contribution-guide additions remain in the working tree with existing planning edits. | Finish native gates, update tested final launch/signing instructions, rerun appropriate final checks and write the requirement-by-requirement completion review with M11 handoff. |

## Current native handoff

### Approved follow-up (2026-11-12)

The owner approved foreground API access and then explicitly approved a
candidate server switch and M9 GUI restart. Inspection of the prepared Apple
Development helper/candidate passed again with zero Keychain calls. Request
mode returned `gui:available`, `mcp:available` and
`credential_values_changed:false`. This establishes only the helper's read;
it does not establish persistent access for the application.
The owner reports choosing “Always Allow” for both native dialogs.

Candidate Start returned `bootstrap_denied`. The ordinary bundle's Status
also returned `bootstrap_denied` before any owner shutdown. Its on-disk
executable hash differs from the original manifest, while the old GUI and
server were still running at that path. The prior unchanged-original rollback
assumption therefore no longer holds for this session.

After the approved graceful SIGTERM shutdown of those identified M9 processes,
the preserved Apple candidate started an owner, but attachment still returned
`bootstrap_denied`. Its GUI visibly reports “Server access denied. Resolve
credentials before connecting again.” Remote controls are disabled. The
[sanitized screenshot](evidence/m9-signed-access-denied.png) records this
blocked state; it is not connected-workflow acceptance. No credential reset,
namespace replacement or permission bypass was attempted. At that point,
persistent native access and rollback recovery remained unproven; the application-path follow-up
below subsequently recovered routine API access.

The follow-up implementation moves the fixed-role foreground reader into a
shared native module and adds the explicit application mode
`--server-api-access --inspect|--request-access`. This permits native approval
of the actual app executable. It dispatches before runtime/profile/GUI creation,
requires an Apple-backed signature, accepts no account or namespace arguments,
and is never invoked by startup or credential recheck. The standalone helper
reuses the same reader. The implementation alone closes no native gate;
subsequent application-path approval and UI-off access results are recorded below.
The new preserved Apple candidate passes strict bundle verification and its
application `--inspect` mode reports `signature_verified:true`, both roles
`not_requested`, `keychain_access_requested:false` and
`credential_values_changed:false`. Its inspection log is
`/private/tmp/samsung-m9-api-access-inspect.log`.

### Actual-application access and fixture review

The owner explicitly approved request mode for the new Apple application.
It returned both roles `available`, with `credential_values_changed:false`.
Routine candidate Start then authenticated the earlier owner and returned
`incompatible_build`; explicit Restart switched to the new candidate and
returned `serving/ready`. This is successful existing-role access without a
credential reset. Native foreground approval is distinct from routine access:
subsequent startup, status, restart and GUI use kept Keychain UI disabled.

The [connected-empty screenshot](evidence/m9-signed-connected-empty.png)
records authenticated attachment without claiming a TV is ready. Native GUI
exit left the same server epoch serving, and reopen attached automatically.

A short-lived TLS fixture was then bound to this Mac's own private interface.
The GUI observed the expected synthetic name/model and the generated
certificate fingerprint, Pair reported “TV approved and saved securely,” and
one remote action produced exactly one `synthetic_key_received` event. No
physical TV was contacted. The fixture log establishes receipt of that action;
no terminal GUI write-receipt assertion is inferred from it.

After an explicit server restart and GUI exit/reopen, the saved selection
restored, the fixture reported `reconnect_authorized`, and the GUI reported
“TV remote connection became ready.” The
[restored screenshot](evidence/m9-signed-fixture-restored.png) contains no local
address or credential. A fresh Check followed by re-pair again reported secure
save. These observations establish native synthetic credential save/read/update
and profile restore for this candidate.

The owner then explicitly approved a cross-build restore and cleanup. The
preserved earlier Apple executable (different build hash, same signing team)
restarted successfully, restored the TV credential written by the new build,
and reached remote readiness with a fixture `reconnect_authorized` event.
Returning to the new candidate again passed authenticated restart and restore.
No second remote action was issued during either restart.

The approved GUI Forget reported “credential removed; trust removed; saved
record removed” and returned to “No saved TVs yet.” The
[cleanup screenshot](evidence/m9-signed-fixture-cleanup.png) records that result.
The fixture exited after `quit`. The new candidate remains `serving/ready`
with an empty fresh profile and its API credentials intact. The configured
signer and ordinary bundle were not replaced. The active candidate is
`target/m9-candidate.X0fccg/Samsung TV Remote.app`, built from the validated
working tree rather than a final committed release.

These results establish synthetic TV credential save/read/update/delete and
cross-build read/restore under Apple signing. They do not establish Wake,
locked/interaction-required credential handling, full signed visual parity or
OS Local Network consent/denial. Existing API role values were not updated or
deleted; API changed-build CRUD evidence remains the earlier disposable probe.

The owner confirmed that only this Mac is available. There is no separate
controlled LAN host for the required OS Local Network review. Same-Mac or
loopback fixture results cannot close that gate. M9 remains incomplete.

### Follow-up lifecycle and documentation review

The signed Settings window displayed the shared-impact Stop confirmation:
every client disconnects, accepted operations may still finish, and the
window stays open. The [confirmation screenshot](evidence/m9-signed-stop-confirmation.png)
records that wording. The confirmation was cancelled; no Stop or Resume was
performed in this follow-up. The first-button handler was also checked against
source and the existing fake-client regression: it stores the current epoch
and returns no task, so displaying the warning does not request Stop.

The [GUI migration review](milestone-09-review.md) assembles the evidence and
unfinished platform cases. It remains a draft until the owner decides whether
those cases stay in M9 or transfer to M11 as release blockers. No scope change
or completed platform result is inferred from the repeated completion request.

### Earlier inspection baseline

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
