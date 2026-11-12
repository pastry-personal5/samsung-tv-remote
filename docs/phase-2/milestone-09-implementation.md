# P2-M9 implementation ledger

Status: In progress. Client-only source cutover is prepared; signed production acceptance remains open.

## Accepted predecessor and current gates

The accepted M8 handoff is [completion review](milestone-08-review.md),
[wire contract](milestone-08-wire.md), [bootstrap transcript](milestone-08-bootstrap.md)
and [implementation evidence](milestone-08-implementation.md). Reuse its
`control_client/{http,background,reconnect}` and
`server/{launch,bootstrap,lifecycle}`; retain its process, listener-proof,
revocation, no-replay and bounded-client regressions.

Implementation begins with signed feasibility. `SignedPeer` obtains its
requirement from the running executable, validates its signature and rejects
ad-hoc signing. A connected peer must have the same effective UID and pass
Security's dynamic code validity check using the complete kernel audit token
from `LOCAL_PEERTOKEN`, including pidversion. No path, PID or requirement
supplied by a client is trusted. `examples/signed_peer_probe.rs` exercises
mutual same-executable acceptance and rejection of `/usr/bin/nc` from the same
user. Existing bootstrap tests own listener proof, transcript context binding,
nonce replay, challenge expiry, rotation and revocation. Platform results must
be recorded separately from compilation and fake-verifier tests.

`ServerKeychain` disables Keychain interaction in the credential-owning process
and uses fail-on-interaction queries. It distinguishes locked, denied,
interaction-required and unavailable from genuine item-not-found. First-use
creation uses Add, which cannot overwrite an item created by a concurrent
owner. `examples/credential_probe.rs` restricts experiments to a fixed
disposable service and canonical v4 UUID accounts; separate processes exercise
create/read/update/delete without printing values. This is feasibility tooling,
not final-identity production acceptance.

Signing cannot distinguish GUI mode from server mode of the same executable.
It also cannot prove human intent or prevent a malicious same-user caller from
invoking an authentic executable. Mode ownership is an application/build
boundary; the signed verifier establishes executable identity only. A stable
certificate-backed designated requirement permits peer authentication across
signed rebuilds while build compatibility remains independently bound by the
bootstrap transcript. Keychain access has an additional platform gate; equal
requirements alone did not pass the self-signed rebuild comparison below.

## Preliminary evidence (2026-11-12)

Validation ran on macOS 27.2.1 (26B131), Darwin 27.2.0 arm64. Signed runs used
the existing local identity with normal host signing/socket/Keychain access;
no identity, trust setting, production profile or legacy item was changed.

| Command / fixture | Observed result |
| --- | --- |
| Unsigned `cargo run --example signed_peer_probe` | Failed closed: `signed identity required`. |
| `zsh scripts/run_m9_signed_feasibility.sh` | Passed signature verification for both fixtures. Same executable accepted mutually; distinct same-user `/usr/bin/nc` rejected. Separate signed credential create/read/update/read/delete processes passed, including refusal to overwrite on duplicate create; synthetic item deleted. UI was disabled for all credential requests. |
| `cargo test --test control_wire` | 5 passed, including omitted/default-false and explicit-true idle flag, codec round trip, malformed flag rejection and rejection on other routes. |
| `cargo test --test control_http idle_launch_connect` | 2 passed. Two-client HTTP tests preserve ready/Connect/Pairing/Wake competitors; idle success is retained across duplicate lookup/submission without a second effect. |
| Required Cargo checks after the idle-Connect change | Formatting and Clippy passed; full `cargo test` exited 0 with 274 tests across 22 suites (including 114 library and 11 server-process tests), zero failures. |
| Required Cargo checks after typed receipt decoding | Formatting, all-target compilation and Clippy passed. Full `cargo test` exited 0 with 275 tests across 22 suites, zero failures. Focused response tests: 7 passed; rotation test: 1 passed, including typed stale-epoch non-admission with zero discovery effects. |
| Required Cargo checks after tracker/scheduler recovery | Formatting, all-target compilation and Clippy passed. Full `cargo test` exited 0 with 288 tests across 22 suites, zero failures. Includes 9 tracker, 2 reference-projection, history/late-admission UI tests and two-window HTTP/SSE recovery in the rotation fixture. |
| Required Cargo checks after injectable client port | Formatting and Clippy passed. Full `cargo test` exited 0 with 291 tests across 22 suites, zero failures (130 library tests, all integration suites, 11 server-process tests). Three fake-client tests cover real Iced task submission, offline suppression, management guards and close-window detach. |
| Required Cargo checks after guarded GUI workflows | Formatting, all-target compilation and Clippy passed. Full `cargo test` exited 0 with 302 tests across 22 suites, zero failures (141 library tests, all integration suites, 11 server-process tests). Eleven `client_workflow` cases cover both continuation orderings, newer pending edits, selection/configuration conflicts, unknown/reconnect, idle-only launch, setup expiry/late responses and exact cleanup/re-pair transactions. |
| Required Cargo checks after lifecycle controls and namespace foundations | Formatting, all-target compilation and Clippy passed. Full `cargo test` exited 0 with 313 tests across 23 suites, zero failures, including all 12 server-process tests. Five GUI lifecycle cases cover confirmation, epoch replacement, durability/settlement, fresh-state barriers and exit. Process tests cover two-client sticky Stop/reopen/explicit Resume; credential rotation tests prove one read-only recheck cannot restore revocation. Temporary-directory tests cover production initialization and cross-namespace record/endpoint/Stop rejection. Captured full log: `/private/tmp/samsung-m9-cargo-test-profile-lifecycle.log`. |
| Required checks on committed client-only cutover `ec3d84d` | Formatting, Clippy and GUI boundary negative corpus passed. Full `cargo test` exited 0 with 317 tests across 24 suites, zero failures, including all 13 server-process cases and the credential rotation/recheck fixture. Four superseded in-process GUI tests were removed after their API cases passed. Captured full log: `/private/tmp/samsung-m9-cargo-test-ec3d84d.log`. |

The peer fixture's actual requirement was:

```text
identifier "dev.samsungtvremote.local.m9-signed_peer_probe" and certificate root = H"e3354ad1a53c821483150d5431286337bb61b309"
```

This is a public certificate fingerprint, not a credential. The final app will
derive its own designated requirement; the fixture identifier is not installed
in production. No final app bundle, signed rebuild/update, locked/denied-store
experiment or Local Network result is claimed here. Preliminary credential
restart success does not waive those final-identity gates.

The idle-only Connect extension is implemented in the shared command, strict
route DTO and native codec. The owner checks readiness and active workflows
before creating an effect, retaining a typed busy rejection. Explicit Connect
defaults to its existing behavior. The API probe now consumes one launch
opportunity after authenticated restore, attempting only an idle disconnected
saved selection and permanently skipping ready/busy/empty states. Reattachment,
Resume and restart cannot repeat it. The current default GUI now uses the same
API path; the legacy runtime adapter has been removed. Release acceptance
still requires the final signed gates below.

## Isolated native lifecycle review (2026-11-12)

The current debug GUI probe ran against a fresh temporary fake profile with
no production or legacy access. A sandboxed launch could not reach macOS
WindowServer/XPC and produced no image; retrying with normal desktop access
produced this [renderer capture](evidence/m9-gui-lifecycle-connected.png).
The connected status fits above the existing UI and explicitly separates
server attachment from TV readiness. No TV was selected or contacted.

A disposable app wrapper around the same debug binary enabled native CUA
interaction. Stop displayed the shared-impact confirmation and kept the
window open. Cancel returned to connected without stopping; confirming Stop
reported durable acknowledgment and observed settlement independently.
Opening Settings while stopped retained that result and disabled discovery,
TV checks and Pairing. Explicit Resume from Settings restored connected state
and enabled discovery/checking again; Pairing remained disabled without a
current observation. These layouts were visually inspected in both windows.
No pairing, remote or Wake effect was requested in this review.

This is development UI evidence, not a signed production bundle, credential or
Local Network acceptance result. Distinct owner-starting/attaching status and
the final signed composition/rebuild, blocked credential and LAN gates still
need their production implementation and evidence.

## Signed production startup evidence (2026-11-12)

Source/configuration commit `ec3d84d` removes the default GUI's runtime path.
The required checks above were rerun after committing. `make bundle` rebuilt
`target/bundle/Samsung TV Remote.app`; strict signature verification passed.
The final application requirement is:

```text
identifier "dev.samsungtvremote.local" and certificate root = H"e3354ad1a53c821483150d5431286337bb61b309"
```

Before first production access, no prior Samsung GUI/server process was live
and the new production root was absent. The legacy profile contained one
device plus trust records. Hashes were retained privately for comparison;
no device, host, token or file contents were printed or copied.

Opening the signed default bundle started its detached same-executable server
without a manual start command. The same signed binary authenticated through
bootstrap and returned `serving=true`, `startup=ready`, API `v1`. The actual
`control-v1` child has the correct namespace and both non-secret API presence
markers; `data/devices.json` is absent, so the new profile is empty. Legacy
device/trust hashes remained unchanged. This proves new API provisioning/read
with the server's noninteractive Keychain adapter and fresh-profile isolation;
it does not prove the remaining TV credential CRUD or blocked-store cases.

An explicit signed CLI Stop durably acknowledged, then the server exited while
the GUI process remained alive. Explicit Resume started a new epoch with the
same empty profile and working protected API access. An ordinary differently
signed Python process sent a valid Hello to the production bootstrap socket;
it received only `Error(denied)`, with no challenge, bearer or proof material.
The legitimate signed client remained serving/ready afterward. No TV command,
Local Network permission change or legacy credential access was requested.

Production visual acceptance is still unverified. CUA returned
`cgWindowNotFound` for the exact bundle, including after server settlement and
a fresh automation session; a Dock lookup timed out. A process sample showed
the GUI in Iced/AppKit's event loop, and explicit current-process/status checks
confirmed the GUI and resumed server remained alive without a restart.
These observations do not prove a visible, usable production window. Do not
substitute the earlier development screenshot for the final production gate.
The live signed GUI/server are retained for the subsequent signed rebuild and
native acceptance checks, with the legacy hashes still unchanged.

## Namespace manifest

Production paths below are reserved for the cutover composition. The new
`server::production::ProductionProfile` provides the checked directory and
server-only service composition. Production `--server` selects it after signed
identity verification. Names remain stable across builds; source wiring alone
is not evidence that the actual profile or credential gates pass.

| Artifact | Legacy | M8 fake | M9 production |
| --- | --- | --- | --- |
| Profile root | `~/Library/Application Support/Samsung TV Remote` | explicit existing parent plus `m8-fake-v1` | `~/Library/Application Support/Samsung TV Remote Control/control-v1` |
| Durable devices/trust | `devices.json`, `trust.json` at legacy root | `data/devices.json`, `data/trust.json` | `data/devices.json`, `data/trust.json` |
| Recovery and pairing backup | `recovery.json`, `pairing-backup.json` at legacy root | under `data/` | under `data/` |
| Owner lock/endpoint/Stop | no detached server | `owner.lock`, `endpoint.json`, `stopped.json` at fake root | same filenames at production root |
| Namespace tag | absent | `namespace` containing `m8-fake-v1` | `namespace` containing `control-v1` |
| Bootstrap socket | absent | `/private/tmp/str-m8-<uid>-<profile-hash-prefix>/bootstrap.sock` | `/private/tmp/str-control-v1-<uid>-<profile-hash-prefix>/bootstrap.sock` |
| TV credential service | `dev.samsungtvremote.local.pairing` | synthetic presence files only | `dev.samsungtvremote.local.control-v1.pairing` |
| TV credential account | canonical `dev_<decimal-u128>` | same ID with `.synthetic` suffix | canonical `dev_<decimal-u128>` |
| API credential service/accounts | absent | memory only | `dev.samsungtvremote.local.control-v1.api`, accounts `gui`, `mcp` |
| Non-secret API provisioning presence | absent | absent | `api-gui.present`, `api-mcp.present` at profile root |
| Feasibility credentials | none | none | excluded: `dev.samsungtvremote.local.m9-disposable`, `probe-<v4-uuid>` |

No legacy data is copied, deleted or selected. The old GUI must be stopped
before the new server opens the reserved production namespace. A rollback
stops and settles the new server before reopening the old GUI. Neither
profile is erased by failed feasibility or failed cutover.

## GUI parity inventory (captured before removing InProcess)

The current entry points are `ui_message::Message`, `app::update`,
`app::complete`, `remote_probe` and `view_model`. Every API mutation also carries
the observed server epoch and a new UUID key. The table records migration
obligations; existing in-process coverage is not API parity acceptance.

| Intent | Request and captured facts | Outcomes and continuation | Evidence to retain/extend |
| --- | --- | --- | --- |
| First authenticated ready snapshot | Connect: selected target/generation, `only_if_idle=true`; consume opportunity once, including empty/ready/busy cases | Connected/ConnectFailed/rejected; no race retry, reattach or Resume continuation | existing `gui_launch_connect_is_explicit_once_and_does_not_select`; add two-client owner guard and fake-client startup tests |
| Pointer/keyboard remote and Power | Remote action captures target/generation; disconnected Power navigates to Power locally | Written/uncertain/not-sent with exact policy; no inferred TV state or replay | shortcut/view-model tests, headless runtime dispatch tests; add fake-client intent/outcome tests |
| SelectSaved/UseCandidate for saved host | Select captures target and expected generation | Saved receipt must have compatible terminal selection/generation/config before optional draft save and Connect | extend receipt-before-event, external selection and same-device new-generation coverage |
| Discovery | Discover with client-bound result handles | Discovered candidates or permission/unavailable failure; no automatic probe | existing discovery/runtime tests; typed client candidates and expiry tests |
| Manual/candidate/check selected TV | Probe or ProbeCandidate with exact host/handle | Probed observation or typed failure; set confirmation only for originating local host/selection context | existing observation/ownership runtime tests; invalidate on external selection and epoch change |
| ConfirmAndPair/ConfirmAndRepair | Pair captures observation, host, pin, explicit matching pin confirmation, replace target, generation and optional recovery transaction | Paired or detailed PairFailed/partial cleanup; no automatic approval or resumed confirmation | `pairing_runtime`, `control_wire`, fake-client re-pair/recovery assertions |
| Wake draft changes | SetWake captures target, generation and observed config revision; draft belongs to that target/config | Saved receipt clears only the submitted draft; edits made during save stay dirty; changed generation/config stops Select → save → Connect | `wake_drafts_survive_resnapshot_and_coalesce_while_storage_is_fenced`; add dirty external selection, pending-save edit and conflict tests |
| Wake | Wake captures target/generation | Every Wake stage/failure including packet boundary; no local animation claim | headless Wake tests, fake-client stage projection |
| CancelWake | CancelWake captures observed Wake operation/target/generation | Cancelled/before/during/after send; preserve packet fact | headless cancellation tests and typed client terminal tests |
| ForgetSelected | Forget captures target/config revision/explicit confirmation | Forgotten with individual store facts; partial cleanup stays actionable | `cleanup_remains_actionable_after_partial_forget_removes_selected_record`, `forget_recovery` |
| Explicit cleanup/re-pair recovery | RecoverForget/RecoverPairing captures transaction, target and confirmation | durable per-store recovery or blocked facts; never infer deletion from local removal | `forget_recovery`, `pairing_runtime`, client terminal decoding |
| Server Stop/Resume/restart/recheck | dedicated management path; shared-impact confirmation for Stop; one pending request | durability distinct from settlement, sticky Stop, mismatch, blocked credential and unsafe endpoint; no TV mutation | M8 process/lifecycle tests, add GUI management and teardown tests |
| Close main/settings window | local lifecycle only | main exit cancels client work, never drains/Stops server; settings close preserves observation/navigation | existing window tests, process detached-lifetime tests |

`terminal_before_admission_reply_is_recovered_without_duplicate_feedback`
must be ported to typed API events/receipts. Initial private recent-operation
history may display retained outcomes but never enters these local
continuations. The single background tracker must own retained lookup;
`App::lookup` and the dual service/probe completion paths must disappear at
cutover. The API path now receives typed outcomes and guarded continuations described
below; the old service/probe completion and Iced lookup paths are removed.

## Remaining implementation and acceptance

Typed receipt parsing now eagerly decodes every wire outcome into
`NativeOutcome`, with explicit failure and not-sent enums, Wake stages and
per-store cleanup facts. Receipts retain target, generation, expiry and the
owner-captured terminal selection/configuration state. Missing terminal
preconditions, unknown outcome/error codes, cross-operation setup handles and
contradictory packet/cleanup facts are rejected at the client boundary.
`SubmitError::NotAdmitted` preserves a known rejection code; malformed error
responses remain ambiguous and retain the original lookup key. The probe uses
the typed failure accessor and no longer receives raw result JSON.

The background client now owns one bounded tracker and lookup scheduler.
It reserves 64 local-intent rows and 64 read-only reference rows in the same
registry, plus a 64-ID settled cache. Restored history cannot displace an
unresolved local request, and a new local request cannot discard restored
history. Only epoch/key/operation reference and delivery state are retained;
mutation bodies are never stored for replay. Unique internal tickets fence
late replies, including pending receipts arriving after terminal settlement.
Known facts remain in the bounded registry until the observer channel accepts
their delivery. Versioned acknowledgments prevent an old pending delivery from
consuming a newer terminal result. Cancelling a GUI callback or a lookup pass
cannot lose a known terminal or precise non-admission result. A known terminal
captured before a changed epoch is reported separately as earlier history,
without a workflow continuation. An epoch change settles unresolved local work
as unknown once. Timeouts and
malformed lookup replies remain unresolved; only typed unavailable/restart
responses or an authoritative epoch change settle uncertainty.

The atomic initial snapshot's private references are bounded, unique and
kind-checked. Shared operation-change events are hints for private lookup,
never terminal facts themselves. Unowned shared references are pruned without
settling a local key. A saturated reference share triggers atomic resubscription
instead of silently losing a new event. Same-epoch resubscription refreshes
read-only references while retaining unresolved local keys. Key association
precedes history recovery, preventing a lost local admission reply from
turning its result into a restored workflow. The scheduler makes at most four
concurrent read-only requests, continues consuming SSE independently, and
coalesces event notifications with its single periodic lookup timer.

The probe no longer launches Iced operation lookup tasks. It merges recovered
local receipts monotonically and handles restored history through a distinct
event that cannot clear drafts, set confirmations or start continuations.
`control_client::port::NativeClient` now provides an injectable interface for
submission, observation, detach, Stop, Resume, restart and protected recheck.
Iced messages carry typed client management errors instead of server
launcher errors. Fake-client tests poll actual Iced tasks without a runtime,
TV, socket or credential store; they cover captured remote/Wake/cancel/Connect/
selection requests, unique UUID keys, offline suppression and duplicate
management requests. TV submissions are suppressed while management is pending;
the close-window fake verifies detach without a TV or management request.
The API probe's `workflows` module now captures each local intent's selection,
configuration revision, setup version, edit version and originating chain.
Select → save dirty Wake configuration → Connect advances only with the
originating receipt's captured terminal preconditions and a compatible atomic
snapshot. Receipt-before-event waits for that snapshot; event-before-receipt
cannot authorize a continuation by itself. Newer generations or unseen config
revisions stop the chain, including same-device reselection. Automatic follow-up
Connect uses the owner-side idle guard as well as captured target/generation.

Wake drafts retain their original target/generation/configuration and newer
edits made while a prior save is pending. A successful matched save can coalesce
those newer edits into a separate save, with no stale receipt clearing the new
draft. Unknown/failed writes and external changes stop automatic continuation.
Conflicted edits stay visible; “Discard draft and reload current settings” is
the explicit recovery action. Reload does not issue a mutation and is blocked
while current server settings are unavailable or storage is settling.

Setup results carry a local version as well as selection context. Same-device
generation changes, epochs and newer probes invalidate earlier confirmations;
Pairing consumes the current explicit confirmation once and rejects expired
checks locally. Discovery, candidate probing, Pairing/re-pair recovery, Forget
and partial cleanup use typed outcomes and exact client-bound/transaction
handles. Native outcome messages retain not-sent reasons, packet-stage facts
and per-store cleanup details instead of generic probe success strings.

The API GUI now confirms the shared impact of Stop against a captured server
epoch. The launcher independently checks that epoch before submitting its one
Stop intent. A durable acknowledgment, matching marker after a lost reply and
observed owner settlement are separate typed facts; persistence errors remain
visible through subsequent checks. Main-window close detaches without Stop,
and late callbacks cannot reopen the client. Resume/restart and one explicit
read-only recheck disable TV mutations until a fresh authenticated snapshot
and connected state arrive. Recheck uses the single attachment loop and cannot
start a missing server, clear Stop, rotate credentials or restore revoked access.
Retained Wake observations stop animating while server observation is unavailable.

The production profile foundation adds a distinct `control-v1` envelope to
devices, trust, recovery and pairing backups. Development, production and
untagged legacy records reject one another, including copied records. Endpoint
and Stop stores have explicit namespace selection, and bootstrap socket paths
use a separate production prefix. The development wrappers keep their original
namespace. `ServerPairingCredentials` fixes TV storage to the fresh pairing
service; it cannot accidentally use the API/disposable service. These adapters
are now selected exclusively by production server mode. The client constructs
only its launcher/bootstrap boundary. Production path selection comes from the
OS effective user's home, so the detached child needs no inherited HOME value.

The default GUI and isolated development probe now share these API flows.
`main.rs` constructs no application services or adapters; `InProcess`, runtime
observer/admission/completion branches, Iced operation polling and runtime
drain-on-close are removed. `development_probe` controls only window titles
and the opt-in debug renderer capture. The required GUI boundary check scans
presentation modules and default composition, including a negative corpus for
runtime/adapters, aliases, raw JSON/receipts, wildcard imports, file access and
external module paths. Its renderer I/O exception requires both fake-mode and
debug guards and does not exempt adapter imports.

The production API store records non-secret `api-gui.present` and
`api-mcp.present` markers. Genuine first-use absence may create an item, while
a missing previously provisioned item, malformed bytes, denied/locked access
or a raced Add never rotates/replaces a secret. Initial role enrollment after
an initially blocked store is atomic and separate from explicit rotation.
Bounded revocation tombstones prevent enrollment from restoring revoked
roles, including revocation before the store first becomes available. An
existing valid API item is read without update; bootstrap still proves the
HTTP listener before disclosing its reusable bearer. Recheck sends no TV
mutation, and only never-enrolled roles can finish initial enrollment.

Distinct Starting/Attaching status is now emitted from actual launcher
ownership work. Unsigned/ad-hoc production initialization fails before profile
access and displays a signed-bundle requirement with TV mutations disabled.
The final signed bundle, changed Wake guidance and production lifecycle still
need native acceptance. The complete checklist remains in the
[overview](milestone-09-overview.md). Signed native screenshots, final bundle
credential restart/rebuild tests, controlled-LAN consent/denial/recovery,
mandatory GUI boundary checks and a completion review remain required.
The optional real-TV smoke runbook is not an exit gate.

Commit `5ee2a9f` consolidates presentation command, admitted-operation and
workflow context into one 64-row, local-tag container. Known admission cannot
regress on a late error or mismatched operation ID, and terminal/unknown
settlement removes one row. Restored history retains precise failure and
per-store cleanup facts without creating a local continuation. The transport
tracker remains the only lookup/recovery scheduler; no presentation retry or
replay queue was introduced. The boundary check passes. Committed full checks
for this and the constrained credential probe passed with 323 tests across
24 suites; native acceptance and the completion audit remain open.

## Signed rebuild investigation (2026-11-12)

Commit `8964d54` adds a server-only credential acceptance command restricted to
`m9-probe-<canonical-v4-uuid>` accounts in the actual fresh pairing/API services.
It verifies its signed identity before store construction, uses known synthetic
values, prints only typed status and keeps Keychain UI disabled. It cannot name
active `gui`, `mcp` or `dev_<decimal>` accounts, and read/update/delete reject
unknown values. Fake tests cover non-overwrite, typed blocked stores, input
scope and repeat cleanup. Create/read passed natively with the local signer.

`make bundle-candidate` creates each candidate in a new directory, preserving
a live older executable. Candidate signature/DR validation passed and the old
bundle hash stayed unchanged. Against the live original owner, candidate Start
returned authenticated `incompatible_build` without replacing it. Explicit
candidate Restart stopped that owner and started a candidate owner, but then
returned `bootstrap_denied`; both old/new clients were denied. This is a failed
M9 rebuild gate, not successful update evidence.

The failed candidate was gracefully terminated with the fresh profile empty.
Starting the unchanged original bundle restored serving/ready access without
reset, credential replacement or ACL changes. Legacy hashes, namespace, API
presence markers and empty new-profile data were preserved. The candidate can
create/read its own disposable items. Equal designated requirements alone
therefore did not establish access to the earlier build's API items.

Apple's [securityd client identity implementation](https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/clientid.cpp)
assigns unclassified signed code a per-build CDHash partition, while supported
Apple developer signing uses a Team ID. Its
[partition enforcement](https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/acls.cpp)
is additional to normal ACL validation. This is a plausible explanation, not
yet a direct diagnosis of these items: narrowly filtered local security logs
contained no matching records. No partition/ACL bypass or reset was attempted.

The controlled comparison is complete. `d1d77db` aligns native bundle version
1.0.0/build 2 for the second signed build. Both comparisons used strict-valid
signatures, equal designated requirements within each signer, and different
executable hashes/CDHashes. All credential operations kept Keychain UI disabled:

| Signer | Changed-build read | Changed-build update/read/delete | Cleanup |
| --- | --- | --- | --- |
| Local self-signed, no TeamIdentifier | Denied in both fresh service scopes | Not attempted after denial | Original creating candidate updated/read/deleted its own items; subsequent read reported missing |
| Apple Development, equal nonempty TeamIdentifier | Passed in both fresh service scopes | Passed in both scopes | Subsequent read reported missing |

Only disposable accounts were used. The configured signer, live original
bundle/server, API role values, namespaces, default keychain and legacy hashes
remain unchanged. The original owner was restored after the failed restart.
These results support Apple developer signing for the next candidate; they do
not prove access to the current self-signed owner's existing role items.

Commit `0a396a6` adds an explicit foreground access-handoff example. It is
separate from the shipped executable; routine server queries still use
UI-off access. Inspection verifies that helper and candidate have the same
Apple-backed identity without calling Keychain. Only explicit request mode
can read the existing `gui`/`mcp` items in the fresh API service and ask for
native approval. It cannot create, replace, delete or print credential values,
name a TV/legacy account, unlock a keychain or alter trust. Denial/cancellation
of the first role stops before requesting the second. The two fake helper
tests pass, as do all 323 package tests, formatting, all-target Clippy and
the 5-case GUI boundary self-test. Its builder passed native strict signature,
same-identity inspection and self-signed/unsigned negative checks. Inspection
reported both roles `not_requested`, `keychain_access_requested:false` and
`credential_values_changed:false`. Original executable and legacy hashes stayed
unchanged; the original server remained serving/ready. A malformed inline
codesign requirement argument was fixed before this successful artifact build.
Final committed checks at `0a396a6` passed: formatting, all-target Clippy,
323 package tests across 24 suites, and both explicit example tests. The full
package output is retained privately in
`/private/tmp/samsung-m9-cargo-test-0a396a6.log`. Changed documentation has clean
whitespace and all 38 checked local links resolve. The inspection artifact's
private manifest records the source revision, public hashes/signature identity,
zero Keychain calls and no credential changes.

No request-mode run or live signer switch is authorized or claimed here. The
owner has been asked to approve Apple signing and the native access handoff.
Existing role items remain intact;
success on disposable items does not satisfy the live-profile update gate.

## Remaining acceptance tooling

The [acceptance audit](milestone-09-acceptance-audit.md) maps all twelve overview
checklist items to current source/test/native evidence and their unproven
parts. It is not a completion review. Read-only revalidation still finds the
original GUI/server alive and serving/ready, with unchanged bundle, handoff
and legacy hashes. CUA inventory reports the app running but exact-path
window inspection still returns `cgWindowNotFound`; no restart follows from
that observation failure.

The audit identified a missing real-adapter synthetic TV workflow. The new
[native fixture](milestone-09-native-fixture.md) serves the Samsung TLS metadata
and WebSocket routes with fixed synthetic identity/credentials, bounded
connections/frames and foreground approve/deny/offline controls. It never
contacts a TV, sends Wake, opens profiles or accesses Keychain. Its own
automated checks use ephemeral loopback and generated certificates. Native
Pairing/Wake/restore acceptance must still run through the selected signed
bundle, and the OS Local Network gate still needs a separate controlled LAN
host. No production target policy is relaxed to accept loopback and no such
host has been identified or contacted. Physical TV smoke remains optional.
The fixture's delegated checks passed all 8 tests, including actual TLS
Pairing/reconnect, denial, key/Ping/Close, malformed frames/headers and
offline/recovery with redacted logs. Syntax, CLI help and forbidden binding
checks passed. The complete log is retained privately at
`/private/tmp/samsung-m9-tv-fixture-tests.log`. These results validate the
fixture only. The owner has been asked whether a separate controlled LAN host
is available; no answer or access is assumed.
