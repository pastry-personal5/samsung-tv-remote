# P2-M7: Headless Control Runtime

Status: Done (2026-11-12), under the recorded M7 entry exception.

## Ownership and composition

`Runtime::start(AppServices, RuntimeLimits)` starts one Tokio owner without an
Iced event loop. Composition retains its lifetime handle and creates trusted
`ControlService` handles with stable `ClientId`s. Authentication and revocation
at an external boundary belong to M8; request payloads cannot choose a client.
A new credential for the same logical identity must reuse that identity.

`control_service.rs` defines commands, immutable snapshots, receipts with
original epoch/target/generation metadata, lookup outcomes and events. These
application types import no presentation, HTTP, MCP or macOS modules. Epochs
are fresh UUID-backed values; operation IDs are monotonic opaque indices
within an epoch. M8 must map these types to the proposed wire representation,
not serialize Rust debug output or assume that a local index is a UUID.

`runtime.rs` owns admission, restore, serialized storage, supervision, the
registry and fan-out. Its private `runtime_setup.rs` and
`runtime_connection.rs` modules implement preparation/commit and connection/
Power/Wake stages. The internal `TvControlCoordinator` retains only session,
remote FIFO and inference policy; it is inaccessible to presentation.
The former synchronous coordinator restore, duplicate cleanup, setup runners,
connect runners and presentation-driven dispatch paths have been removed.

`main.rs` composes the existing adapters with `LocalRecoveryJournal`.
`presentation/iced/in_process.rs` owns the temporary in-process lifetime and
client handle. The GUI holds projections, local drafts/navigation, pending
client requests and wording. Its one event reader attaches atomically and
resubscribes after overflow; retained-result queries close the admission-reply
race and recover results after resnapshot. Neither queries nor GUI animation
ticks pump TV effects. Closing Settings leaves admitted work and drafts intact.
Launch reconnect is one explicit Connect for the attached epoch/selection/
generation, without an implicit Select. Valid Wake edits coalesce before
admission while storage is fenced; a rejected action is not blindly replayed.

This is still one GUI process. Detached launch, HTTP, MCP, sticky Stop/Resume,
external authentication and surviving GUI exit belong to M8–M11. M7's Done
status does not accept M6's pending signed Keychain/process experiments.

## Admission, ordering, and events

Admission checks epoch and input bounds, then a client/key duplicate before
registry quota or mutable policy. An unchanged duplicate returns the original
receipt even at capacity or while draining; a changed payload conflicts.
Current-epoch policy failures are retained. Ingress, malformed input, registry
and per-client registry refusals have no reserved operation or new effect.
Each admitted operation reserves a result and terminalizes once. Late workers
reconcile facts without changing an already terminal result.

The registry indexes keys and operation IDs and expires terminal results with
a monotonic FIFO. It does not scan every record for each lookup or admission.
Results survive observer disconnect/acknowledgment for 30 minutes. Old-epoch
lookup is `UnknownAfterRestart`; missing same-epoch lookup is
`OutcomeUnavailable`. Neither permits automatic replay.

The sole remote FIFO has one writer. Dispatch rechecks target, generation,
Pairing and readiness; Play/Pause resolves from owner inference at that point.
Selection fences cancel waiting work and preserve uncertainty for an active
write. Old/duplicate session events cannot complete a current request.
Uncertain writes and lost sessions reset inferred playback. A 136-second total
remote lifetime bounds queue residence plus the existing four-second write.

Snapshot capture and subscriber registration are one owner transition.
Every active subscriber receives each consecutive global revision, including
opaque operation ID/coarse phase for another client. Only the owning client
can look up detailed outcomes and client-bound observations. Snapshots share
immutable profile/revision arrays. Overflow drops the sender; EOF requires a
fresh atomic subscription. No retained replay log is promised.

Completion delivery uses the bounded JoinSet itself (at most eight tagged
results), separate from ingress, plus a 32-event session lane and a dedicated
drain signal. This replaces the design's proposed generic 32-completion
channel. Drain/completions/timers have priority; at least every 16 owner
transitions also services ingress so session pressure cannot starve queries.
No semaphore has an unbounded queue of pending effects.

## Persistence and recovery

All synchronous preference, trust, journal and credential calls run on
reserved blocking workers. There is one serialized storage lane. Deadlines
terminalize the original operation but retain the worker slot and fence until
actual completion. The owner does not wait on a worker-held storage mutex.
Selection/configuration publish durable changes only after the worker returns;
ambiguous errors reread durable facts before releasing the lane, or block on
an unreadable profile. Single-file selection/configuration need no cross-store
journal: atomic replace plus file/directory sync and authoritative reread
provide reconciliation; restart loses the original outcome, not the file.

Pairing prepares approval/token/session in memory. Before commit, the owner
rechecks client-bound live observation, host/full pin, generation and recovery
fences. A reserved first-Pairing ID is journaled before token, trust, device or
selection writes. Re-pair and reconnect rotation use the same journaled commit.
Stale preparation drops its secret/session without saving. Only a committed,
fresh session can become Ready, including during another selection fence.

`recovery.json` contains only IDs, kind, stages, prior selection and cleanup
flags. It never contains a token, raw certificate or host. Re-pair retains the
previous non-secret device/trust data in the owner-only single
`pairing-backup.json` slot, whose transaction is bound to the marker. One backup
slot also bounds orphaned data after a crash before preparation. Existing
`devices.json` v1/v2, `trust.json` v1, saved IDs and Keychain service/account
identities are unchanged. Marker/store replacement and marker removal sync
the parent directory. No secret is backed up to a file.

Restore reads both marker kinds before normal control, without TV I/O,
credential retry or duplicate-host deletion. Corrupt/unknown markers, invalid
backups and unreadable/oversize profiles fail closed. Recovery actions are:

| Block | Explicit guarded action |
| --- | --- |
| Partial Forget | `RecoverForget` with exact device, transaction and confirmation; retry only incomplete deletions. |
| Interrupted first Pairing | `RecoverPairing` with the same guards; remove only its reserved ID and restore a still-valid prior selection. Each compensation stage is journaled. |
| Interrupted re-pair/rotation | Fresh Probe and exact-fingerprint Pair approval with the exact recovery transaction and target. Reuse the original metadata/trust backup until the new transaction completes. A normal repeat cannot bypass the marker. |
| Corrupt/unrecognized files or backup | Block the profile; preserve files for explicit diagnosis/backup. Do not infer credential deletion or retry denied credentials on startup. |

Forget durably clears selection before deletion. Its partial result retains
per-store facts and the marker even if the device record is gone; Settings
provides an independent cleanup action. Re-pair cannot restore an overwritten
old token from non-secret evidence, hence the fresh-approval rule. These tests
model persistence boundaries and actual temporary files, not power-loss
filesystem behavior or signed Keychain access.

## Power, Wake, and drain

Disconnected Power captures the active Wake configuration at admission and
checks a paired connection first. Reachability becomes Ready without a key or
packet; only network Offline/Timeout permits the one captured packet. Missing
credentials, certificate failures and storage timeout never imply offline.
Wake edits affect later operations, without cancelling the captured packet.
Selection/Forget/drain revoke pending permits. Explicit cancellation names
the exact Wake operation as well as its selected target/generation; a stale
cancellation cannot revoke a later Wake. Packet sending is supervised
without dropping an underlying blocking DNS job. Cancellation before its final
send check revokes the permit; cancellation during a possible send remains
uncertain, and cancellation after confirmed send retains that fact.
Reconnect retries only eligible failures, at 500 ms, inside the original
30-second readiness deadline, including storage stages. It never resends.

Drain immediately closes new admission, aborts the session, terminalizes queued
work as not-sent and possible writes as uncertain, and reports within five
seconds. Read-only queries and known duplicates remain available while
workers settle. `DrainHandle::status` reports unresolved storage/effects;
`settled()` must finish before another owner reuses that profile. Dropping the
handle detaches supervision instead of aborting blocking work. A marker
survives any process interruption in a cross-store commit. GUI process exit
itself is still the M7 lifetime boundary, not background-server evidence.

## Bounds, timing, and verification

Production defaults and the saturation run are recorded in the
[M7 completion review](milestone-07-review.md). Tests use small overrides,
barriers and paused Tokio time, and never a real TV or credential item. The
review links every verification-matrix case to concrete tests, records measured
heap/stage timings and qualifies the remaining platform evidence.

Run the [required Cargo checks](../contribution-guide.md#required-validation)
from the repository root. The M8 handoff is `Runtime`, `ControlService`, typed
commands/metadata/errors, atomic subscriptions, `DrainHandle`, explicit limits,
`tests/common` fake profile fixture and the runtime/recovery integration suites.
M8 must reuse this owner and use isolated storage/fake credentials until the
M9 production-profile cutover gate.
