# P2-M9: GUI API Migration

Status: Planned

## Baseline and entry contract

The inspected GUI currently starts `InProcess` from `AppServices`, receives
application `Snapshot`/`Receipt` values and drains its runtime when the main
window closes. `main.rs` constructs preferences, Keychain, Samsung, SSDP and
Wake adapters before launching Iced. M8's working-tree `server/control_http`
contains wire/request code, not an accepted complete server/client/lifecycle
handoff. Do not equate those types with delivered HTTP behavior.

Before implementation consumes the handoff, record the accepted M8 DTO/error
fixtures, event framing, lossless ID/counter rules, retention/lookup semantics,
request limits, timeouts, credential capability model, protected attach
transcript and launcher/Stop/Resume interface. Reuse M8's client and retry
policy rather than creating a second HTTP stack or Iced reconnect loop. If a
required field or route is missing, resolve it in the shared contract with
fixtures; do not query storage or reconstruct server authority in the GUI.

## Ownership and module shape

Use an MVVM-inspired Iced boundary without two-way binding:

| Boundary | Responsibility |
| --- | --- |
| `control_client` outside `presentation/iced` | Typed HTTP requests, authentication, bounded SSE decoding, one attach/reconnect lifecycle, retained lookup and safe client events. No TV/storage adapters. |
| Presentation projection/request tracker | Map safe client DTOs to display state; retain observed preconditions and pending operation correlation. No server policy or transport loops. |
| Iced `App::update` | Coordinate local navigation/windows, drafts, explicit confirmations and client tasks. |
| Views and view model | Render projection, messages and advisory enablement; emit UI intents. No HTTP calls or runtime ownership. |
| Executable composition | Dispatch mode before adapter construction; server mode owns production TV/storage ports, GUI mode owns only client/bootstrap ports. |

Exact file splits follow M8's accepted client placement. Keep the interface
small: attach/observe, submit typed intent, lookup by operation ID/client key,
and explicit lifecycle management. Expose typed transport, authentication,
protocol and server rejection errors rather than strings or raw HTTP objects.
Share safe DTO definitions, not a `ControlService` handle. UI messages carry
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
Within an epoch apply consecutive revisions only. Ignore duplicate/older
snapshots; a gap, explicit resync requirement or broken stream invalidates live
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

Allocate a unique client key once per user intent before sending, scoped as
specified by M8 to stable client identity and server epoch. Do not retain the
current process-local `gui-{sequence}` pattern across GUI restarts unless the
accepted key scheme proves it cannot collide. Bound submitting/pending maps
and terminal lookup work using the accepted client/server budgets.

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

Receipts, terminal events and lookups may race. Merge them monotonically by
operation identity, complete once and prevent a late admission receipt from
reopening terminal progress. After reconnect query outstanding IDs/keys in a
bounded way; an event gap triggers resnapshot rather than guessed completion.
After GUI restart show only server-provided retained history/private outcomes;
do not add a persistent client replay queue. Changed epoch makes old unresolved
work unknown, even if the new TV state happens to resemble success.

## Workflow migration and parity

| GUI flow | API and projection obligations |
| --- | --- |
| Startup/restore | Attach to server-owned restore state. Empty fresh profile sends no Connect. For an eligible restored selection, preserve the existing launch Connect as one observable API intent after first attachment; consume that launch intent once and never repeat it on stream reconnect. Honor recovery/credential blocks. |
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
Power. M8's management protocol handles durable Stop, drain/settlement and lock
release; the GUI does not unlink markers, kill a PID or select another port.
Keep blocked states pending explicit recovery rather than reconnect-driven
credential prompts. Rotation reattaches through the protected channel with a
fresh credential/snapshot; revocation stays blocked. Resume/restart schedules
immediate attach but never repeats the launch TV Connect intent.

## Signed composition and fresh profile

Keep one locally signed app executable with early `--server` dispatch. Move
production adapter construction out of normal GUI startup into server
composition. First server launch creates a distinct empty profile/credential
namespace, even with populated legacy fixtures. Later launches restore only
devices paired into that new profile. Leave legacy data and credentials intact;
no migration or synthetic legacy-Keychain probe belongs to this milestone.

Implement M8's protected bootstrap/peer-verification port with the selected
signed macOS identity mechanism before enabling production composition. Record
the concrete code requirement/audit identity and transcript binding to profile,
endpoint and build; UID, PID and socket permissions alone are insufficient.
A same-executable code identity does not isolate GUI mode from server mode;
document that limit rather than claiming signing enforces mode separation.
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

1. **Freeze handoff and parity inventory:** name accepted M8 artifacts and
   unresolved signed gates; map current UI messages/outcomes to API fixtures.
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
   management actions, detach on close; implement signed peer/credential ports
   and verify fresh-profile/Keychain/Local Network behavior in the bundle.
6. **Atomic cutover and review:** remove `in_process` and old composition; make
   the GUI dependency check mandatory, run required checks and native review,
   update current architecture/UX/contribution instructions and record evidence.

## Verification matrix

All entries below are required planned evidence, not passing test claims.

| Area | Evidence |
| --- | --- |
| Projection | Initial snapshot, duplicate/old events, consecutive revisions, gaps, changed epoch, stale callback, event-before-receipt, receipt-after-terminal and local navigation preservation. |
| Request formation | Fake-client assertions for every parity row, observed preconditions/confirmation, unique keys across GUI lifetimes, draft conflict and zero fresh-key retry after loss. |
| Recovery | Fake HTTP server drops before/after admission and during terminal response; key/ID lookup, unknown/expired/private outcomes, bounded cleanup and no automatic mutation replay. |
| Reconnect | Fake-clock full sequence/repeated hourly cap, flapping/reset, one timer/attempt, sleep/exit, Stop/Resume, mismatch, rotation and credential blocks. |
| Cross-process | Two authenticated clients change selection; GUI subprocess exit preserves server, outage/restart yields a new snapshot/epoch with honest outcomes. Use fake TV/credential ports. |
| Boundary | Required check covers all GUI modules and normal composition, rejects runtime/coordinator/storage/TV construction or imports, and proves fake profiles cannot select production ports. |
| Signed platform | Committed bundle configuration/build identity, peer rejection/replay, empty profile despite legacy fixtures, first Pairing, Wake, close/reopen, credential lifecycle/rebuild, prompt counts and Local Network denial/recovery. Record screenshots/sanitized observations. |
| Release | Old GUI stopped, one production owner, all flows API-only, no fallback; required Cargo commands and current documentation match the tested artifact. |

The completion review must distinguish deterministic fakes, actual subprocess
HTTP evidence, signed macOS observations and target-TV checks. Missing signed
or hardware results remain explicit blockers/deferred evidence as owned by the
milestone checklist; do not mark M9 Done from implementation alone. M10 gets a
working GUI client for cross-client testing; M11 gets the signed bundle,
boundary check and remaining session/upgrade/accessibility/hardware matrix.
