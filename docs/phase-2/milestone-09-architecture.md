# P2-M9: GUI API Migration

Status: In progress; client-only source prepared, signed release acceptance remains gated

## Baseline and entry contract

At the M8 handoff the default GUI started `InProcess` from `AppServices`,
received application `Snapshot`/`Receipt` values and drained its runtime when
the main window closed. `main.rs` constructed TV/storage adapters before Iced.
M9 has now removed that path: the current default GUI uses only the API client,
and production server mode constructs the adapters. Signed release acceptance
remains open; see the implementation ledger. M8 is Done: its isolated `--gui-probe` uses the
existing window over actual HTTP/SSE without a local TV runtime. That probe
is the migration starting point, not shipping parity or signed acceptance.

Freeze the handoff against the [M8 completion review](milestone-08-review.md),
[wire contract](milestone-08-wire.md), [bootstrap transcript](milestone-08-bootstrap.md)
and [implementation evidence](milestone-08-implementation.md). Reuse
`control_client::{http,background,reconnect}`, `server::{launch,bootstrap,lifecycle}`
and their fixtures. The existing client already has UUID v4 keys in the probe,
64 tracked intents, cancellation-to-uncertain recovery, concurrent lookup/SSE,
a five-second attach deadline and a cancellation-safe 35-second stream idle
deadline. Preserve these regressions while extending the client; no second
HTTP stack or Iced reconnect loop is needed.

At the M8 handoff, `projection` reconstructed application snapshots and left
terminal outcomes as JSON; `SubmitError::NotAdmitted` lost the rejection code.
M9's typed receipt work addresses these two limitations; see the
[implementation ledger](milestone-09-implementation.md). Initial private `recent_operations` are not projected;
the presentation still has separate service/probe completion paths and local
lookups. Production peer and credential adapters are wired into server composition;
production dispatch no longer requires `--fake-profile`. The development
namespace still requires that explicit argument and cannot select production. Do not treat existing
method signatures as the final shipping boundary. Missing safe fields belong
in shared DTOs/fixtures, never direct GUI storage access. M6's remaining signed
gates are M9 cutover requirements under its updated acceptance scope; planning
does not mark M6 accepted or authorize a production experiment.

## Ownership and module shape

Use an MVVM-inspired Iced boundary without two-way binding:

| Boundary | Responsibility |
| --- | --- |
| `control_client` outside `presentation/iced` | Typed HTTP requests, authentication, bounded SSE decoding, one attach/reconnect lifecycle, retained lookup and safe client events. No TV/storage adapters. |
| Presentation projection/local intent context | Map safe client DTOs to display state; retain observed preconditions, drafts and local continuation context keyed to the client tracker. No duplicate request registry, server policy or transport loops. |
| Iced `App::update` | Coordinate local navigation/windows, drafts, explicit confirmations and client tasks. |
| Views and view model | Render projection, messages and advisory enablement; emit UI intents. No HTTP calls or runtime ownership. |
| Executable composition | Dispatch mode before adapter construction; server mode owns production TV/storage ports, GUI mode owns only client/bootstrap ports. |

Exact file splits follow M8's accepted client placement. Keep the interface
small: attach/observe, submit typed intent, lookup by operation ID/client key,
and explicit lifecycle management. Expose typed transport, authentication,
protocol and server rejection errors rather than strings or raw HTTP objects.
Share safe DTO definitions, not a `ControlService` handle. Introduce an injectable
client port and transport-neutral client status/error/receipt types; Iced
messages must not expose `server::launch` errors or `serde_json::Value`.
Pure IDs, action enums and validation values may be shared with domain code;
that does not grant access to application services. UI messages carry
client events, immutable preconditions and operation references, never runtime,
TV sessions, `AppServices`, Keychain handles or Samsung frames.

## Projection and stream reconciliation

Separate local presentation state (navigation, focus, message scroll, windows,
address/MAC drafts and dialogs) from authoritative projected facts (devices,
selection/generation, connection, setup/recovery, Wake and operation outcomes).
Keep wire DTOs distinct from display models; the server remains the authority
for queue admission, disconnected Power Toggle, Play/Pause inference, Pairing
and persistence. Disable controls until authenticated readiness and the initial
atomic event snapshot arrive, then use advisory server-derived enablement.

Use the stream's atomic snapshot, not a state GET followed by subscription.
Within a subscription apply consecutive revisions only. M8 events carry full
safe state, not patches or private terminal outcomes. Preserve its decoder's
resnapshot behavior for duplicate/out-of-order wire events; at the presentation
boundary ignore duplicate/older callbacks from an already validated attachment.
A new attachment may legitimately begin at the same revision: install its
atomic private references and readiness even when display facts are unchanged.
A gap, explicit resync requirement or broken stream invalidates live
readiness and reattaches for a fresh snapshot. A changed epoch discards old
revision comparisons, candidate/observation handles and confirmation contexts.
Tag asynchronous callbacks with attachment/epoch and request identity so a
late old response cannot regress the projection or finish a new operation.
Heartbeats and coarse other-client events do not imply local command success.

An external selection change updates the displayed target and invalidates
unsubmitted intents bound to the prior generation. Already submitted work
retains its original target/outcome. Navigation never changes server selection.
Bind Wake drafts to device, epoch, selection generation and configuration
revision. Preserve unsaved text visibly on conflict, but require review before
saving against new facts; never auto-save a stale draft onto the new TV.
Invalidate Pairing confirmation when its address/observation context changes.

## Request identity and outcome recovery

Allocate a UUID v4 key once per user intent before sending, following the M8
probe, scoped to stable client identity and server epoch. The in-process
`gui-{sequence}` pattern must disappear at cutover. Keep a local tag for UI
correlation, but never use it as server identity. Retain the existing 64-intent
client cap and bound completed display/dedup entries separately. Use one client
tracker and lookup scheduler; remove presentation-owned polling and duplicate
pending maps once migrated. Recovery must keep consuming SSE while lookups
wait; cancelling a submitted future preserves its original key for lookup.

Track requests through these distinct facts:

| Fact | GUI behavior |
| --- | --- |
| Locally unsent or authoritative rejection | Show the precise validation/admission failure; no effect claim beyond supplied facts. |
| Submitted, response lost | Retain original key/epoch/target; mark admission/outcome unresolved and query by key after attachment. |
| Admitted receipt | Store operation ID; admission/queueing is progress, not terminal success. |
| Terminal retained outcome | Render the server's not-sent, written, uncertain, observed or workflow result once. |
| Unknown/epoch lost/retention unavailable | End the progress indicator with explicit unknown wording; no automatic resubmission. |

A network error cannot use the current generic “not admitted” message. Resolve
by read-only lookup, never by resubmitting even the same key; retention may
have expired. A user may make a separate explicit intent after reviewing an
unknown result, with fresh observed preconditions. Do not silently turn a
retry button into replay of the old request.

Submission replies, lookup-driven terminal updates and snapshot references may
race. Merge them monotonically by
operation identity, complete once and prevent a late admission receipt from
reopening terminal progress. After reconnect query outstanding IDs/keys in a
bounded way; an event gap triggers resnapshot rather than guessed completion.
After GUI restart decode the initial snapshot's at-most-64 private
`recent_operations` references, then look up their typed outcomes without
inventing the original local command. Identity is shared by GUI instances;
these references are not proof that this window submitted the operation. Show
recovered facts without replaying UI continuations, opening confirmations or
overwriting drafts. Shared `operation_changed` events trigger/coalesce bounded
private lookups; they are never terminal results themselves. Do not add a
persistent client replay queue. Changed epoch makes old unresolved
work unknown, even if the new TV state happens to resemble success.

Transport failures and pending lookup timeouts remain unresolved. Only a typed
unknown response, changed epoch or exhausted documented retention can end
tracking as unknown; lack of progress is not evidence of rejection. Mark local
spinners as waiting for recovery while disconnected, retain target/context,
and release tracking slots after terminal/unknown settlement. Preserve typed
admission error codes and all terminal outcome variants, including Wake send
and cancellation stages and per-store cleanup facts. A generic probe message
or a `kind` string alone cannot meet production parity.

## Workflow migration and parity

| GUI flow | API and projection obligations |
| --- | --- |
| Startup/restore | Attach to server-owned restore state. Empty fresh profile sends no Connect. At the first authenticated startup-ready snapshot, consume the launch opportunity once: Connect only a disconnected saved selection with no Connect, Pairing, Wake, storage/recovery or admission block. A ready/busy/empty target skips it permanently for this GUI lifetime. Never defer it until another client finishes work, or repeat it on reattachment, Resume/restart or selection change. Capture epoch/target/generation; an admission race rejects without retry. |
| Remote, keyboard and pointer | Carry observed target, generation and epoch. Preserve disconnected Power navigation, one-packet Wake policy, playback uncertainty and existing message wording through server outcomes. |
| Selection/connect | Selection is an explicit generation-checked mutation; follow-up Connect uses the confirmed resulting selection, not optimistic local selection. External replacement invalidates a delayed follow-up. |
| Discovery/manual probe | Use client-bound attempt/candidate/observation handles; manual input remains local until submitted. Expired handles require new explicit setup, not invented candidates. |
| Pairing/re-pair | Bind explicit confirmation to the current observation and trust decision. Keep TV approval and recovery/compensation facts; never auto-confirm or retry after interruption. |
| Wake configuration/Wake/cancel | Validate draft format locally for feedback; server validates and persists with config preconditions. Project captured configuration/stages; cancel names the observed Wake operation and target/generation. Local animation is not evidence of a packet or cancellation. |
| Forget/recovery | Use the existing explicit confirmation flow and typed recovery commands. Render partial cleanup/store failures as blocked facts; local removal cannot claim durable deletion. |

Keep a parity inventory against existing UI intent and outcome tests before
removing the adapter. Port meaningful assertions to fake-client tests rather
than copying in-process implementation details. Isolated intermediate builds
may migrate one vertical slice at a time; no intermediate mixed composition
ships or opens the production profile.

Before implementation, inventory `ui_message::Message`, `app::update`,
`app::complete`, the probe completion path and the view-model/shortcut tests.
For each workflow row above record the emitted request, captured preconditions,
every typed success/failure, follow-up intent and test location. Include
Select → save dirty Wake configuration → Connect and editing a draft while
its prior save is pending. Follow-ups require the originating local intent,
the receipt's captured `terminal_state`, and a compatible current snapshot;
matching target alone is insufficient after a same-device generation change.
One completed step cannot complete or restart another step. Conflicted or
unknown chains stop visibly and require a new user intent; restored history
never starts a chain.

Idle-only launch Connect needs an owner-side guard, not just a GUI enablement
check: the current Connect command checks target/generation but can race another
client's connection or Wake within that same generation. Plan a narrow optional
`only_if_idle` flag on the connections request, default false for existing
explicit Connect behavior. Launch Connect sends true; the owner checks current
disconnected/idle facts atomically before starting work and retains a typed
busy/blocked rejection without disturbing an existing session/workflow.
Update the shared command, wire parser/codec, contract/examples and HTTP/client
fixtures together before consuming the extension. Test a second client making
the TV ready or starting Connect/Pairing/Wake between snapshot and admission;
the automatic request must create no competing effect and must not retry.

## Connection and lifecycle UI

Reuse the [M8 reconnect state machine](milestone-08-architecture.md#gui-background-connection-and-reconnection)
with one subscription/attempt/timer. Immediate attach is followed by failure
delays of 1, 2, 4, 8 seconds through 3,600 seconds, repeating at the cap. Reset
only after 30 healthy seconds; sleep causes at most one due attempt. Every
attempt has a bounded deadline. GUI exit cancels client work without cancelling
admitted server operations or sending runtime drain/Stop.

Show starting/attaching, connected, reconnecting with next retry, stopped,
incompatible build, blocked credentials and unsafe endpoint distinctly from TV
connection/Local Network failure. Preserve the last snapshot as visibly stale
while offline, with mutations disabled. Outage must not leave a permanent
Pairing/Wake spinner or falsely report a failed TV write.

Expose lifecycle status/actions through the existing Settings surface and
message area: explicit Stop, Resume when stopped, and explicit restart for an
authenticated incompatible build. These are server lifecycle intents, not TV
Power. Stop requires a confirmation naming its impact on every client; keep
the GUI open with Resume available afterward. Disable duplicate management
requests and TV mutations while management is pending. Report durable Stop
acknowledgment separately from completed settlement; a lost reply/observed
marker must not claim that active work drained. A failed Stop stays visible
without an automatic Resume or another Stop request.
M8's management protocol handles durable Stop, drain/settlement and lock
release; the GUI does not unlink markers, kill a PID or select another port.
Keep blocked states pending explicit recovery rather than reconnect-driven
credential prompts. Rotation reattaches through the protected channel with a
fresh credential/snapshot; revocation stays blocked. Resume/restart schedules
immediate attach but never repeats the launch TV Connect intent.

Add an explicit credential recheck after the user resolves a blocked store;
it performs one protected attach and does not rotate, regrant revoked access,
or issue TV commands. No generic Retry button may hide a mutation retry.
Closing Settings leaves observation active. Closing the main window follows
the existing app-exit policy and tears down only client tasks, including late
management callbacks; an already accepted Stop can still finish server-side.

## Signed composition and fresh profile

Keep one locally signed app executable with early `--server` dispatch. Move
production adapter construction out of normal GUI startup into server
composition. First server launch creates a distinct empty profile/credential
namespace, even with populated legacy fixtures. Later launches restore only
devices paired into that new profile. Leave legacy data and credentials intact;
no migration or synthetic legacy-Keychain probe belongs to this milestone.

Before choosing production paths, record a namespace manifest for profile,
trust/journal files, lifetime lock, endpoint/socket/Stop metadata, TV Keychain
service/accounts and API Keychain service/role identities. Keep legacy,
development and production identities disjoint and stable across rebuilds.
The current zero-sized `KeychainCredentialStore` hardcodes the legacy service
and collapses most errors to `Unavailable`; it cannot be reused unchanged.
Only genuine item-not-found permits first-use provisioning. Preserve typed
locked/denied/interaction-required errors without overwriting existing items
or silently generating replacement API secrets. GUI mode may use protected
bootstrap/management ports but never reads TV or API Keychain items directly.

Implement M8's protected bootstrap/peer-verification port with the selected
signed macOS identity mechanism before enabling production composition. Record
the concrete code requirement/audit identity and transcript binding to profile,
endpoint and build; UID, PID and socket permissions alone are insufficient.
A same-executable code identity does not isolate GUI mode from server mode;
document that limit rather than claiming signing enforces mode separation.
Launching an authentic executable is also not proof of human authorization;
do not claim protection against arbitrary malicious same-user invocation.
If this process shape cannot enforce the accepted trust boundary, record the
design change and threat-model limit before release. Fakes cannot accept that
production gate. Do not send an API bearer to an unverified HTTP listener.

Keep signing requirements stable across a signed rebuild/update. Test new TV
and API credential save/read/update/delete, restart and update with the signed
bundle; count routine prompts separately from initial consent, locked/denied
store errors, TV approval and Local Network consent. Block denied/locked or
interaction-required stores visibly without treating them as an empty profile
or retrying prompts. Verify the detached server's actual Local Network access,
including denied and restored permission; `cargo run` is not bundle evidence.

Before first production-profile access, stop the prior single-process GUI and
prove single ownership. Atomically ship all GUI flows on the API path; no
failure fallback starts an in-process runtime. A failed cutover retains both
profiles and reports the gate; it does not import, erase or reset data. A
rollback must stop/settle the new server before launching an older app, with
its legacy profile left intact. M11 hardens broader installation/upgrade and
uninstall scenarios.

## Implementation sequence

1. **Freeze handoff and signed feasibility:** name accepted M8 artifacts,
   finish the parity inventory and namespace manifest, and prototype signed
   peer verification/noninteractive fresh credentials with disposable items
   before the broad migration. Record the exact identity requirement and
   blocked cases. Failed feasibility revises the design before cutover work.
2. **Client projection and tracker:** add typed safe events, epoch/revision
   reconciliation, unique keys, lost-response lookup and bounded tracking.
   Prove ordering/races with fake clients before migrating interactions.
3. **Read/control slice in isolated builds:** state/restore, external selection,
   one-time launch Connect, remote/shortcuts and Wake/cancel. Keep every effect
   observable through the API and test stale-target and uncertain outcomes.
4. **Setup/configuration slice:** discovery/probe, confirmations, Pairing/re-pair,
   Wake draft conflict handling, Forget and explicit recovery. Finish every
   parity row with deterministic request/outcome tests.
5. **Lifecycle and signed composition:** reuse M8 backoff, add status and explicit
   management actions, detach on close; finish signed peer/credential ports
   and verify fresh-profile/Keychain/Local Network behavior in the bundle.
6. **Atomic cutover and review:** remove `in_process` and old composition; make
   the GUI dependency check mandatory, run required checks and native review,
   update current architecture/UX/contribution instructions and record evidence.

## Verification matrix

All entries below are required planned evidence, not passing test claims.

| Area | Evidence |
| --- | --- |
| Projection | Initial/repeated same-revision attach, private recent references, duplicate/old callbacks versus invalid wire order, consecutive revisions, gaps, changed epoch, stale callback, event-before-receipt, receipt-after-terminal and local navigation preservation. |
| Request formation | Fake-client assertions for every parity row, observed preconditions/confirmation, unique keys across GUI lifetimes, draft conflict and zero fresh-key retry after loss. |
| Recovery | Fake HTTP server drops before/after admission and during terminal response; key/ID lookup, unknown/expired/private outcomes, bounded cleanup and no automatic mutation replay. |
| Reconnect | Fake-clock full sequence/repeated hourly cap, flapping/reset, one timer/attempt, sleep/exit, Stop/Resume, mismatch, rotation and credential blocks. |
| Cross-process | Two authenticated clients change selection; GUI subprocess exit preserves server, outage/restart yields a new snapshot/epoch with honest outcomes. Use fake TV/credential ports. |
| Boundary | Required check covers GUI modules, messages/tasks and composition dependencies; rejects runtime/coordinator/storage/TV access, aliases/re-exports and local service construction. Permit pure domain/client DTOs and a narrow executable mode dispatcher. Negative fixtures prove forbidden paths fail; production mode never selects development peer/storage adapters, and fake profiles cannot select production ports. |
| Signed platform | Committed bundle configuration/build identity, peer rejection/replay, empty profile despite legacy fixtures, fixture-backed first Pairing/Wake, close/reopen, credential lifecycle/rebuild, prompt counts and controlled-LAN Local Network denial/recovery. Real TV smoke is supplementary. Record screenshots/sanitized observations. |
| Release | Old GUI stopped, one production owner, all flows API-only, no fallback; required Cargo commands and current documentation match the tested artifact. |

The completion review must distinguish deterministic fakes, actual subprocess
HTTP evidence, signed macOS observations and target-TV checks. Missing signed
or hardware results remain explicit blockers/deferred evidence as owned by the
milestone checklist; do not mark M9 Done from implementation alone. The
[real-TV smoke runbook](milestone-09-tv-smoke.md) is a documented follow-up,
not an M9 exit gate; record not-run/deferred results honestly. Signed identity,
Keychain and Local Network checks remain required, using controlled LAN
fixtures where appropriate without claiming target-TV evidence. M10 gets a
working GUI client for cross-client testing; M11 gets the signed bundle,
boundary check and remaining session/upgrade/accessibility/hardware matrix.
