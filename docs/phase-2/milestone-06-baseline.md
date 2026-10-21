# P2-M6 Phase 1 baseline and migration map

Status: Reviewed against the worktree on 2026-10-15. The acceptance column
describes future evidence, not a passing claim. The shipped application is
still one process with direct Iced-to-coordinator calls.

## Current data and bundle inventory

| Artifact | Current version or identity | Preservation and acceptance |
| --- | --- | --- |
| `devices.json` | `version: 2`; reader accepts v1 and v2; selected ID, saved UUID/label/host and Wake settings; no config revision | Keep UUIDs, selected ID and valid Wake data. The new per-device config revision is epoch-scoped in memory and starts at zero on restore; an epoch change invalidates old preconditions. Back up the file byte-for-byte before any other migration; test v1 and v2 fixtures with the new server and rollback with the old GUI. |
| `trust.json` | `version: 1`; saved ID to host and certificate SHA-256 | Preserve full pin and host binding; fail closed on corrupt or missing trust. Back up before mutation. |
| Keychain generic password | Service `dev.samsungtvremote.local.pairing`; account is saved device UUID | Never export to JSON or logs. Test synthetic old-GUI-created item read/update/delete under each signed server shape, after upgrade and with locked/denied Keychain. Development uses a distinct test service or fake port. |
| Application Support | `~/Library/Application Support/Samsung TV Remote` | Server is sole writer only after atomic GUI cutover. Test rollback against a backup, not a profile concurrently opened by old and new binaries. |
| Bundle | `scripts/build_macos_bundle.sh` builds one signed GUI executable, bundle ID `dev.samsungtvremote.local`; current plist has Local Network usage text | M6 signed prototype must use the actual local signing identity and record min macOS version, process shape, Local Network attribution and Keychain prompts. M9 adds the chosen server executable/mode. |
| Dispatch journal | In-memory `RemoteDispatcher`, caller acknowledges terminal results | Replace observer-driven removal with client-scoped operation registry; test two clients and bounded expiry. No on-disk operation replay. |

`LocalDeviceRepository` writes via temporary file and `sync_all` on the file,
then rename. It does not currently sync the parent directory. Pairing saves a
Keychain token, trust, device, then selection; re-pair overwrites token before
trust/device; forget attempts all three deletions. These are not atomic across
stores. The current in-memory `PartialCleanup` result does not survive a crash.

## Behavior migration

| Current owner / behavior | Target owner | Acceptance evidence |
| --- | --- | --- |
| `App::boot` restores selection and queues `ConnectSelected` | Server restores safe data without TV I/O; GUI issues explicit connect after attach | MCP-only start produces zero discovery, connect, Pairing, Wake or key calls in fake ports. GUI startup retains current reconnect intent. |
| `TvControlCoordinator` and `ControlState` hold selection, generation, Pairing, connection, playback inference | One server state owner | Two clients see one selection; same-ID explicit selection increments generation and restarts session; stale generation never writes. |
| `RemoteDispatcher` owns one FIFO, one in-flight write and one-observer journal | Server owner keeps one FIFO; registry retains per-client results | Saturation, stale queued write, uncertain in-flight write, duplicate key and expiry tests. |
| `SamsungGateway`, `TvSessionControl`, session event stream | Supervised server effect/session tasks | A late event tagged with old session/epoch cannot change current state. A socket write is reported only as written. |
| SSDP and secure port-8002 probe | Server discovery/probe effects | Bounded candidate and observation handles; certificate confirmation binds exactly one fresh attempt. |
| `TvSetupService` Pairing/re-pair and reconnect material | Server setup policy with durable recovery fence | Crash injection after each Keychain/trust/repository step leaves affected ID blocked until safe reconciliation; old records survive failure. |
| `TvSetupService::select_saved` and coordinator `select_saved` | Durable server selection command | Compare-and-swap expected generation; storage failure leaves old generation; successful same-ID reselect increments it. |
| `TvSetupService::save_wake_configuration` | Server config command | Explicit ID/config revision prevents stale overwrite; MAC validation and no packet on invalid config. |
| Coordinator Power Toggle and Play/Pause inference | Server action policy | Disconnected power probes then attempts one Wake packet only after offline/timeout; other errors do not Wake. Play/Pause is resolved just before write and stays unknown after uncertainty. |
| Coordinator Wake plan/cancellation | Server tracked Wake operation | Packet-send and remote-ready facts stay distinct; cancellation after send does not relabel it unsent. |
| Coordinator forget and `ForgetResult` | Server fenced forget and recovery marker | Per-store result is visible; partial removal blocks target across restart; no false complete result. |
| Iced `App::update` schedules TV effects and receives typed completions | GUI client calls plus server events; server runs effects | No direct Samsung, SSDP, Wake, Keychain, preferences or coordinator import in GUI after M9. |
| `src/main.rs` constructs all TV/storage adapters | Server composition root constructs them; GUI constructs only control client | Build/source boundary check and signed bundle process review. |

The existing fake-port tests in `tv_control_coordinator.rs`,
`remote_dispatcher.rs`, `tv_setup_service.rs`, and `app.rs` are the
characterization baseline. They include uncertain playback, disconnected
power, Wake cancellation, stale session results, storage failure, and partial
forget. M7 must retain those facts while moving the owner; M8–M11 add
cross-process and multi-client coverage. Hardware and signed-bundle checks
are separate from deterministic fake tests.

## GUI state ownership

| Present `App`/`Message` state | Future owner and mapping |
| --- | --- |
| Window IDs, `PrimaryView`, settings page, shortcut focus, message feed resize/scroll | GUI local state; Iced messages remain GUI events. |
| Address text, Wake MAC drafts/active choice, settings status/dialogs | GUI local draft until a validated request is sent. A server snapshot never overwrites an unsubmitted draft silently. |
| `view_model` control projection | GUI copy tagged by server epoch/revision; invalid on stream gap or restart. |
| `coordinator`, `AppServices`, selected/saved state, generation, Pairing/connection/Wake state | Server authority; GUI reads snapshot and projects it. |
| `AttemptRemoteAction`, `PowerToggle`, `Wake`, `CancelWake`, `DiscoverTv`, `ProbeTv`, `ConfirmAndPair/Repair`, `ConnectSelected`, `ForgetSelected`, `SelectSaved`, Wake edits | GUI intents mapped to typed `ControlClient` requests with preconditions, then versioned HTTP DTOs. None is itself a server command type. |
| `DiscoveryFinished`, `ProbeFinished`, `PairFinished`, `ConnectFinished`, `PowerProbeFinished`, `WakeSent`, `WakeReconnected`, `ForgetFinished`, `SessionEvent`, `StaleSetupCleaned` | TV effect completions move to server owner; GUI receives operation receipts and revised events instead of `SessionPackage`/transport objects. |
| `WakeTick` | Local rendering timer only, if still needed; cannot decide whether a packet was sent. |

`ControlClient` is the GUI boundary. It maps a UI intent to a typed client
request and maps wire DTOs back to safe projection facts; it does not own a
second queue or retry policy. The server adapter maps DTOs to application
commands. The server owner alone admits work and emits results/events.

### One full user-action sequence

1. The GUI receives atomic snapshot `(epoch E, revision 12, TV A,
   generation 7)`. The view model projects A/7 and enables Volume Up.
2. The user clicks it. Iced emits `Message::AttemptRemoteAction` (or a
   presentation-only successor). The GUI reads **target A, E and generation
   7 from that projection**, generates operation key K once, and calls
   `ControlClient::remote_action(A, E, 7, K, VolumeUp)`.
3. The client maps to `POST /v1/remote-actions`; the HTTP adapter validates
   and maps to the application command. The owner authenticates the client,
   reserves the registry/FIFO slot, and returns an operation ID receipt.
4. The GUI displays “queued for A” and retains K. It does not display “TV
   volume changed.” The server rechecks A/7 before its one socket write.
5. The session completion reaches the owner. It records `written` or
   `uncertain`, advances revision 13, and emits an operation event. The GUI
   maps the event to “sent to A” or “A may have received the command.”
6. If the HTTP result or event is lost, the GUI looks up K under its own
   identity. A new epoch or expired record yields unknown wording and an
   explicit new user action is required before another key is sent.

## Data recovery contract for M7–M9

Use an owner-only durable non-secret journal in the Application Support
directory, with one record per affected device: transaction ID, operation
kind (`first_pair`, `repair`, `forget`), saved device ID, stage, and prior
selected ID where needed. Generation is per server epoch and is not restored.
Write and fsync a `prepared` marker
*before* the first Keychain, trust, or repository change; fsync the directory
after atomic rename. Keep the marker until all stores and selection commit
or all compensations complete. The journal contains no token, raw
certificate, private text, or host. A crash at any later step leaves a marker
and forces `blocked_partial_setup` for that device on restart, before any
connect, Wake, key, re-pair or forget retry can run.

For first Pairing, retry idempotent removal of the reserved ID's token,
trust, and device only after noninteractive store access succeeds. If the
new ID had become selected, restore the journaled prior selected ID only if
its saved record is still valid; never automatically reconnect it. If any
step is denied or ambiguous, keep the marker and require explicit recovery.
For forget, repeated deletes are idempotent, but the server must confirm all
stores before clearing the marker and reporting complete. For re-pair, a
crash after overwriting the old Keychain item can destroy the old token; a
non-secret journal cannot restore it. Preserve the old metadata/trust backup,
block control and require a fresh, TV-approved re-pair if the transaction
cannot be proven complete. Never roll back metadata while silently using an
unknown new token. A Keychain locked/denied/interaction-required result stays
blocked without a timer or reconnect retry loop. Backup and rollback of
non-secret files are separate from Keychain access migration.

Crash injection must stop the process after journal preparation, each store
write/delete, selection write, and journal removal for all three operations.
On restart, fake ports prove that no blocked ID is used for TV I/O, partial
forget never reports complete, and a completed marker can be cleared safely.
The current code has no journal; this is an M7 implementation gate and an M9
real-profile cutover gate.
