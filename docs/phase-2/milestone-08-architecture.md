# P2-M8: Control HTTP Server

Status: Done

This plan is implemented for the explicit isolated fake profile. The default
GUI production cutover remains M9 and MCP remains M10. See the
[completion review](milestone-08-review.md),
[implementation record](milestone-08-implementation.md) and
[wire decisions](milestone-08-wire.md) for tested behavior and evidence.
The [M6 contract](milestone-06-contract.md) owns wire semantics;
[M7's review](milestone-07-review.md) owns the implemented runtime limits.

## Entry and publication gates

M7 is Done under its recorded exception. On 2026-11-12 the owner selected
same-executable `--server` mode with on-demand background launch and removed
device-key migration, including the synthetic legacy experiment, from scope.
The owner authorized isolated HTTP and process/lifecycle work with fake
credentials; the [overview](milestone-08-overview.md#owner-decisions-and-entry) records
the M8 entry exception and required outcomes, including an early isolated GUI
connection probe. Signed fresh-credential behavior still requires evidence
for the selected identity in M9. No legacy migration experiment blocks M8.
M6 final acceptance remains separate. Production profile use remains prohibited
before M9's GUI ownership cutover, which initializes the fresh profile.

The owner's review decisions on 2026-11-12 keep the early probe in the existing
GUI with minimal wiring, require an explicit restart for an incompatible
server build, and close existing SSE streams on API credential rotation.
Reconnect uses the new credential and a fresh atomic snapshot; it never
resubmits a mutation.

Before publishing `/v1`, reconcile these concrete M7/M6 differences in the
contract, examples and MCP corpus, then add compatibility fixtures:

- `Epoch` has private representation; operation IDs are runtime `u64`s and
  candidates combine an operation ID and index, while M6 specifies opaque UUIDs.
  Add narrow parsing/formatting interfaces and an epoch-scoped, reversible wire
  encoding with checked kind/range validation. Specify the exact bit layout,
  canonical spelling and how the enclosing epoch disambiguates IDs across
  restarts; a UUID cannot contain both a full 128-bit epoch and a full 64-bit
  counter reversibly. Validate kind and current epoch before owner lookup;
  encoding is not authorization. Avoid an unbounded ID map or
  deriving wire authority from a supplied client ID. Preserve persisted device
  IDs; do not change `devices.json` to solve transport encoding.
- `SetWake` requires selection generation as well as configuration revision;
  cancellation requires target and generation. Document these required fields.
- `RecoverPairing` and `RecoverForget` are explicit runtime operations needed
  for complete setup parity. Specify guarded recovery routes, exact transaction
  and target confirmation fields, and examples before implementing the routes.
  Never turn a read, startup or reconnect into implicit recovery.
- Map all `Outcome`, `Rejection`, `ServiceError`, startup and recovery variants,
  including terminal Busy, storage uncertainty and supervisor failure. Fix how
  safe expiry/display timestamps are projected without changing monotonic
  retention, and how recent operation references are obtained through the
  façade. Add only the narrow query support actually needed. Terminal expiry
  must be captured once at terminalization and stay stable across duplicate
  replies/lookups, including wall-clock changes; monotonic time alone governs
  eviction. Candidate/observation display expiry never extends their authority.
- Resolve M6's proposed 64 KiB body limit against this plan's 16 KiB budget.
  Adopt 16 KiB only with fixtures proving every maximum valid request fits,
  and update M6 in the same contract slice. Do not leave two active limits.
- Define JSON integer representation for runtime `u64` IDs/counters/revisions.
  Prove lossless handling above JavaScript's exact integer range, and define
  duplicate-field, numeric overflow, unknown-field and non-finite-value errors.
  Shared fixtures must exercise the Rust client and the future MCP boundary.
- Bound recent operation references to this authenticated client's records;
  obtain them in the same owner transition as the snapshot. Do not assemble
  them from a second query or expose another client's private receipts.

An intentional semantic change needs a recorded contract decision. Do not
silently expose Debug output or give the HTTP adapter different policy.

## Fresh-profile policy

The owner selected a fresh, empty device list on 2026-11-12. Give the new
profile a distinct non-secret storage location and credential namespace;
changing only the preferences directory cannot isolate the current global
Keychain service. The runtime restores only this profile, never falls back to
legacy device/trust/recovery or credential data, and does no implicit import.
First launch has no saved or selected TV. Normal restarts preserve devices
added to the new profile; initialization is not a recurring reset. Do not
remove old data as an initialization side effect.

Test first launch with populated legacy fixtures and an empty new profile:
state remains empty and fake ports record zero legacy reads or TV effects.
Then add/pair a device, restart and verify new-profile restoration. M9 must
repeat credential lifecycle checks with the selected signed executable and
new namespace. Keep persisted device-ID formats unchanged within each profile.

Give the M8 fake profile a separate namespace from both the legacy GUI and
M9's future production profile. Fake paired records/trust must never become
production state. Across actual server restarts, recreate only deterministic
synthetic pairing material through the fake credential adapter, or retain it
through a dedicated test-only mechanism; never persist real secrets in fixture
files. Verify add/pair, process exit, restore, explicit connect and one action
in a second process. GUI exit/reattach alone proves neither credential nor
profile restoration after a server restart. M8 server/probe compositions must
fail closed on a production-profile request; selecting a path must not enable
real ports.

## Components and ownership

| Planned location | Responsibility |
| --- | --- |
| `src/server/mod.rs` | Server composition and supervision, one runtime per profile. |
| `src/server/control_http/` | Versioned DTOs, explicit conversions, routes, status mapping and SSE. |
| `src/server/access/` | Shared Host/Origin/authentication gate and stable client/capability mapping, reusable by M10. |
| `src/server/lifecycle/` | Lock, manifest, authenticated readiness, detached start/attach, Stop/Resume and drain. |
| `src/infrastructure/macos/` | Credential and bootstrap peer-verification adapters behind testable ports. |
| `src/main.rs` | Dispatch GUI or `--server` mode before Iced initialization; isolated server development uses the same entry path. |
| `src/control_client/` | Narrow development GUI HTTP client, shared wire DTOs, lookup and atomic event subscription; extended to full GUI parity in M9. |
| `src/presentation/iced/` | Explicit isolated GUI probe mode using the HTTP client instead of constructing an in-process TV runtime. |
| `tests/control_http.rs`, `tests/server_access.rs`, `tests/server_lifecycle.rs` | Fake-port HTTP and subprocess evidence. |

The executable is a composition root. Handlers receive a typed `ControlService`
for the authenticated identity, never `AppServices`, a credential store, TV
transport, coordinator or mutable registry. Lifecycle management has a separate
capability-checked interface; it uses runtime drain rather than a second TV
command path. `Runtime::start`, `client`, `drain` and `DrainHandle::settled`
remain the owner/lifetime boundary.

Use the same signed app executable for GUI and server processes, as selected
by the owner. Dispatch `--server` before initializing Iced and keep GUI and
server composition in separate modules. Normal `cargo run` still opens the
GUI; document an explicit isolated fake-profile server command using
`--server`. A second server binary is not needed. No production launch path is
enabled by default in M8. Same-executable modes share a code identity; module
boundaries enforce ownership, not Keychain isolation between the modes.

Choose the HTTP implementation in the first implementation slice after
checking current primary documentation. Record dependency/features and why it
supports bounded parsing, streaming backpressure and shutdown; no crate is
selected by this plan. Keep framework types out of `application` and `domain`.

## Request and response path

1. Enforce connection/header/time limits and loopback peer policy. Accept only
   the allocated literal loopback authority and port; reject alternate Host,
   duplicate authority and proxy-forwarded authority tricks. Bind a literal
   IPv4 address `127.0.0.1`, never a wildcard, IPv6 or DNS alias, consistent
   with the existing M6 endpoint contract.
2. Allow absent Origin for native clients or the exact allocated
   `http://127.0.0.1:<port>` Origin fixed by M6; reject `null` and all others
   before admission. Do not enable wildcard CORS or trust `localhost` DNS as
   identity. Apply the same gate to readiness, reads, writes, events and
   unknown routes. Any stricter policy requires a recorded contract change.
3. Verify the credential and capability before creating a client service handle.
   Identity comes from authentication, never JSON, URL parameters or bearer text
   used as a registry key. Fail closed before any effect on authorization error.
4. Parse bounded versioned DTOs, content type, enums and opaque IDs. Validate
   confirmations and path/body agreement. Reject malformed JSON, unsupported
   fields/version, oversized or incomplete bodies with safe typed errors.
5. Translate once to `Request` and call the service. Preserve owner admission
   ordering: duplicates precede policy/capacity; a new retained policy failure
   carries operation metadata even when its HTTP status is an error. Ingress
   failure or stale epoch cannot invent an operation receipt.
6. Return `202` for pending and the documented terminal status for completed
   work. Disconnect, response timeout or failed serialization cannot cancel an
   admitted action. Lost replies are recovered by same-client key lookup;
   unknown/old-epoch outcomes never authorize automatic replay.

Implement every route in M6's route table, including Stop, and the explicitly
reviewed recovery additions. Table-driven fixtures must cover every command
and terminal outcome, not just successful remote actions. Safe DTO projection
must exclude tokens, raw certificates and internal errors; return network
metadata only where the authenticated contract requires it. Suppress sensitive
request/response bodies, Authorization and operation-key paths in access logs.

## Authentication and bootstrap

Use distinct GUI and MCP credentials with stable identities across rotation.
Separate TV-control permission from GUI lifecycle management. Credential
verification must support revocation before the next admission, constant-time
secret comparison, bounded attempts and prompt-free blocked states. Rotation
closes existing streams; clients authenticate again and take a fresh snapshot. Revocation closes streams, discards queued output and prevents
further authorized reads. Bytes already delivered cannot be recalled.

Define one authorization/admission ordering shared by all adapters. Checking a
credential only in middleware before awaiting the runtime mailbox is
insufficient: revocation can finish while that request is still queued. Add a
narrow admission guard or owner-ordered authorization update so requests not
admitted before revocation cannot start afterward. Already admitted work keeps
its factual outcome and is not cancelled by losing its credential. Test a
request paused between middleware and owner admission, concurrent revocation,
rotation and buffered SSE output. Keep credential material outside the owner.

Bootstrap is a protected local channel, never an unauthenticated HTTP token
route. Put peer identity checks behind a port and test denied/incorrect peers.
The M9 signed implementation must verify the selected code requirement/audit
identity, not just PID, socket ownership or `getpeereid`. M8 implements and
tests this boundary through injected peer-verification and credential ports;
its development path cannot silently fall back to production bootstrap. Define
channel framing, challenge freshness, identity-to-capability mapping and replay rejection before
implementing it. A same-UID fake does not establish signed peer authenticity.
The first access slice must record a concrete, reviewed bootstrap/readiness
transcript: roles, secret separation, nonce source/lifetime, bounded outstanding
challenges, identity/profile/endpoint/build binding and replay handling. A
challenge presented by an unknown HTTP listener is not proof of identity. Use
a standard authenticated construction through the protected channel; do not
invent a cryptographic protocol or send a reusable bearer to prove readiness.
The isolated client must use the same verification boundary with fake trust,
not bypass it. If the selected process shape cannot enforce the proposed
identity check, record a design change and its threat-model limit before publication.

Persist API secrets only through the selected credential port. Runtime files
contain endpoint and lifecycle metadata, never bearer or TV tokens. No secret
appears in arguments, environment or stdout. Tests inject credentials in memory;
the future M10 header helper's intentional credential output is outside M8.
Locked/denied/interaction-required stores produce typed blocked status without
retry timers or reconnect-driven prompt loops.

## Endpoint and process lifecycle

Use an owner-only per-user/profile runtime directory with a held advisory lock.
Reject symlinked, foreign-owned or permissive paths/files rather than following
or repairing them blindly. Create files with restrictive modes and atomically
replace and sync manifest/Stop metadata. Test interrupted replacements. PID
metadata is diagnostic only; lock ownership plus authenticated readiness proves
attachment. Keep the lock until the owner and all persistence workers settle.

First allocation chooses a free high loopback port under the lock and persists
the non-secret stable URL (including the future `/mcp` path). Subsequent starts
reuse it. An unrelated occupant or failed challenge causes a collision error;
never kill the process, silently pick a new port or disclose credentials to an
unverified listener. Use a nonce-bound authentication exchange that proves the
listener possesses the server secret before sending any reusable client bearer.
Once allocation metadata is durable, bind failure never silently reallocates.
A bind or crash before that first commit may retry allocation under the lock.
Readiness reports build, API compatibility, epoch and serving state separately
from TV/storage availability. Stale manifests, crashed PIDs and incompatible
builds require explicit tested handling; never launch a second profile owner.
An incompatible authenticated server produces a visible mismatch and requires
an explicit restart; opening the GUI never replaces it automatically. Restart
must use a compatible management path, wait for drain and lock release, then
start the replacement. If management compatibility cannot be established, fail
visibly without killing the old process. An explicit restart is distinct from
sticky Stop and never replays a TV operation.

The launcher starts once with controlled environment, closed inherited handles
and detached lifetime. Concurrent starts attach to the authenticated winner.
No login registration is installed. Client disconnect does not stop the server;
startup restores metadata/trust/recovery only and performs no TV I/O or implicit
Pairing credential reads.

Stop is a management operation with epoch/key deduplication. Serialize it
against mutation admission: validate authorization/epoch/key, close new mutation
admission, durably write the sticky marker, then acknowledge and invoke drain.
Requests admitted before the fence retain their facts; requests after it cannot
start a TV effect. During a pending/failed marker write, report a visible
stopping/blocked state; do not acknowledge durable Stop or silently reopen
admission. The marker includes non-secret identity, profile, epoch and Stop-key
metadata sufficient to resolve a lost reply. Never store a bearer in it.
Preserve uncertain write/packet/storage facts. On marker-write failure, return
a typed failure without claiming durable Stop. Keep lifecycle management
available in that blocked state: an explicit new Stop intent/key may retry
marker persistence and drain; the failed key retains its original result.
Resume is an explicit authorized
GUI launcher action after the old owner releases the lifecycle lock; ordinary
start/attach and MCP cannot clear the marker. The live server holds that lock
for its lifetime: the launcher must not acquire it before requesting live Stop,
or wait for a response while holding a lock the server needs. A concurrent
start loser waits for readiness/lock change with a finite deadline, not by
holding another server's lock. Resume waits for settlement, atomically clears
the marker under exclusive ownership, then launches once. A failed launch after
an acknowledged Resume is a visible launch error; normal start may retry it.

Test Stop racing a launch, Resume and lost replies. A crash before durable Stop
leaves no successful Stop claim; a crash after durable Stop preserves the
marker and must not auto-start. An ordinary crash without a marker creates a
new epoch on recovery. Never clear a durable marker merely because its PID or
epoch is stale. Corrupt lifecycle metadata fails closed with a typed error;
it does not imply permission to start.

The five-second runtime drain report is not proof that blocking persistence
finished. Retain ownership until settlement or process exit; do not let a live
old worker overlap a replacement owner. Signals and logout use the same safe
drain path without accidentally recording intentional Stop. Sleep/wake does not
replay a TV effect. Model these transitions in M8 tests; full signed session,
upgrade and fast-user-switch evidence remains M11-owned.

## Transport resource budget and SSE

Keep M7's measured limits unchanged unless a contract change is justified.
Proposed additional M8 limits, to finalize with saturation evidence before
publication:

| Resource | Initial implementation budget |
| --- | --- |
| Accepted HTTP connections | 128 total; refuse excess without owner admission. |
| Concurrent ordinary requests | 64 total, 16 per authenticated identity; no unbounded waiting queue. Reserve capacity for readiness, queries and authorized lifecycle work. |
| Header / mutation body | 16 KiB aggregate headers / 16 KiB streamed body, enforced without trusting Content-Length. |
| Header / body read deadline | Five / ten seconds; bound slow and chunked senders. |
| Ordinary response write / keep-alive idle | Ten / 30 seconds; query/admission timeout never cancels an admitted effect. |
| SSE write deadline / heartbeat | Ten / 15 seconds; finite write buffers and no whole-history accumulation. |
| Subscribers | M7's 64 total, four per identity, 64 events each; counted within connection budget. |

Bound bootstrap peers, authentication attempts, path/key length and serialized
response size as well. Measure worst-case full-profile JSON/SSE against M7's
1,024-record fixture and select an explicit output cap with headroom; report
failure safely rather than truncating valid state. HTTP handlers must not copy
maximum snapshots into unbounded per-connection buffers. Disable unbounded
pipelining and compressed request bodies; bound JSON nesting, request-target
length, chunk framing and serialization work as well as payload bytes. Test
raw HTTP framing, including conflicting Content-Length/Transfer-Encoding,
duplicate Authorization/Host and slow incomplete headers. Record explicit
HTTP version/keep-alive policy; rely on one parser for framing.

Connection caps alone do not prove availability. Ordinary clients and SSE
must leave bounded capacity for authenticated readiness, queries and Stop;
use finite reservations and response deadlines, and keep runtime drain
independent of the HTTP request semaphore. State the achievable load guarantee
and test it at every limit, including unauthenticated saturation. Do not claim
availability under arbitrary local denial of service. Measure HTTP overhead
separately from M7's approximately 66 MiB peak-heap fixture.

SSE consumes `ControlService::subscribe` directly: emit its atomic snapshot at
revision `r`, then consecutive events. Never read state then subscribe. Preserve
coarse cross-client operation events and owner-only result lookup. On owner
queue overflow, blocked writes, revocation or closed receiver, close the stream;
try `resync_required` only when writable. Heartbeats do not advance revision.
Reconnect and Last-Event-ID use a fresh snapshot, not an invented replay promise.

## Early GUI integration

Use an explicit development option to attach the GUI to an isolated fake-server
profile. Use the existing GUI with only the wiring needed for setup, one action
and reattach, as selected by the owner; a new developer screen and full GUI
migration are outside M8. Select this composition before constructing
`AppServices` or the local runtime. Reuse wire DTOs through `control_client`; views receive safe projections
and operation references. The probe must attach, consume the atomic initial
snapshot/events, establish a fake paired/selected session, submit one tracked
remote action and recover its outcome by lookup. After GUI exit, reattach and
verify the server and new-profile state survived. Stop/Resume may initially be
exercised through the authorized lifecycle harness; shipping GUI management
controls and full flow parity remain M9 work.

This is an early required delivery slice, not a second shipping control path.
Default GUI launch continues using its existing composition. The development
probe and server use isolated storage/fake ports and must never construct two
owners for one profile. Add meaningful tests for composition selection and
cross-process state/action behavior; do not require a real TV or native window
session for automated checks. Include a screenshot if the implementation
changes visible GUI elements, following the contribution guide.

## GUI background connection and reconnection

The owner requires automatic background server attachment and reconnection in
both the M8 development GUI probe and M9's shipping client. Attempt immediately
on GUI launch. For transient connection failures, wait 1 second, then 2, 4, 8
seconds and continue doubling to a maximum delay of 3,600 seconds. Keep retrying
at that one-hour interval while the GUI remains open; one hour is the interval
cap, not a total retry deadline. These are delays after each failed attempt;
each attempt also has a bounded connection/readiness timeout.

Use one asynchronous retry loop, one in-flight attach attempt and one timer
per GUI client. Keep the window responsive and show connection status plus the
next retry time. Establish authenticated readiness and a fresh atomic event
snapshot before enabling control. Reset the delay to one second only after
30 seconds of healthy authenticated event subscription, so a flapping server
does not repeatedly reset the backoff. A stream ending before then counts as
another failure. Use monotonic timers; after sleep, perform at most one due
attempt, never a burst of missed retries. GUI exit cancels the loop without
stopping the server.

Transient failure may invoke the existing on-demand launcher when the server
is absent; the same single-owner and authenticated-attach checks apply on every
attempt. Sticky Stop suspends automatic start/attach until explicit Resume.
An authenticated incompatible build requires explicit restart. Credential
revocation, denied/interaction-required bootstrap and unsafe endpoint metadata
remain visible blocked states rather than timed retries or prompt loops.
Rotation can reconnect automatically once the replacement credential is
available through the protected channel; it must not reuse the revoked secret.
Successful Resume or explicit restart schedules an immediate attempt.

Connection retries only recover the server connection and event projection.
They never resubmit Pairing, selection, Connect-to-TV, Wake, remote actions or
other mutations. Recover outstanding outcomes by retained ID/key lookup;
changed epoch or unavailable results remain honestly unknown. Do not infer TV
readiness from a restored server connection or automatically repeat the GUI's
initial TV-connect operation on each attachment.

Add deterministic fake-clock tests for immediate startup, the complete delay
sequence and repeated one-hour cap, bounded/non-overlapping attempts, flapping
streams, healthy reset, sleep without catch-up bursts, GUI exit, Stop/Resume,
incompatible builds, credential rotation/blocked states and zero mutation
replay. The subprocess GUI probe must include server outage and automatic
reattachment with a fresh snapshot. M9 reuses this client policy rather than
adding a second reconnect loop in Iced messages.

## Implementation sequence

1. **Contract and fresh-profile composition:** resolve publication differences,
   dependency choice and transport limits; add DTO fixtures and dispatch
   `--server` before Iced. Use isolated fake ports, a new profile namespace and
   empty first-use state. Preserve default GUI launch and leave legacy data
   untouched. Verify new-profile persistence separately from initialization.
2. **Access and isolated process shell:** shared gate, fake bootstrap/credential
   ports, secure paths, lock, stable endpoint and readiness. Add on-demand
   detached start/attach and prove parent-exit survival, simultaneous starts
   and stolen-port failure before exposing control operations. Specify and test
   the admission fence and Stop/Resume state machine here, even though full
   race/load hardening comes later.
3. **Early GUI/API slice:** add the narrow development HTTP client and explicit
   isolated GUI composition. Wire state/events, fake setup, one tracked remote
   action and result lookup. Demonstrate exit/reattach with the server still
   running. Keep the adapter extensible for M9's complete GUI migration.
4. **Complete HTTP adapter and events:** cover every read/mutation/recovery and
   outcome through the service; finish SSE privacy, resnapshot and revocation.
   Test two-client selection, deduplication, fake Pairing/Wake failure paths and
   lost replies. No handlers access storage or TV ports directly.
5. **Lifecycle and load hardening:** durable sticky Stop, explicit GUI Resume,
   signal drain, late-worker settlement and incompatible-build handling;
   transport/serializer saturation and responsive queries/drain. Model user
   session transitions. Keep signed peer identity and fresh-credential Keychain
   evidence in M9, and full signed session integration in M11.
6. **Review and handoff:** run required Cargo gates, update contract/examples,
   development commands, architecture and decision log; write an M8 review with
   actual test references, measured bounds and deferred platform evidence.
   Completion requires API coverage, runnable lifecycle and the GUI probe.

## Verification matrix

The matrix below records the original requirements. The
[completion review](milestone-08-review.md) maps every row to actual test and
artifact evidence, including the boundaries of fake and signed verification.

| Area | Deterministic M8 evidence |
| --- | --- |
| Headless parity | Start subprocess with isolated fake profile; no Iced initialization, production directory/Keychain access or startup TV effects; every API command and recovery path invokes the expected fake boundary. |
| Fresh profile | Populated legacy fixtures are never read or imported; initial new profile has no devices/selection/trust/credentials; add/pair then restart restores only new-profile data; no reset or deletion on ordinary startup. |
| GUI reconnect | Fake-clock exponential delays through the repeated one-hour cap, healthy reset, one attempt/timer, sleep/exit, blocked states and no mutation replay; subprocess outage/automatic reattach. |
| GUI probe | Explicit development composition has no in-process TV runtime; attach/snapshot/events, fake setup, tracked action/lookup, exit and reattach preserve server state. Default GUI composition remains unchanged. |
| Admission | Two authenticated identities, same-ID reselect, stale generation/epoch, duplicate and changed keys at capacity, response loss and detach; verify one tracked effect or none and private lookup. |
| Wire contract | Golden request/result/error fixtures for every route and variant; malformed IDs/JSON/enums, confirmations and path/body disagreement; exhaustiveness checks for future variants. |
| Access | Reads/readiness/events protected; wrong/revoked credentials, refresh preserving identity, MCP denied management, Host/Origin variations, proxy headers and non-loopback peers. |
| Bootstrap | Wrong UID/code requirement, stale/replayed challenge, malformed framing, unavailable/locked credential store; no token in files/URLs/logs. Real code identity needs signed evidence. |
| Events | Mutation during subscribe, consecutive revisions across both clients, initial maximum snapshot, overflow, slow reader, disconnect and revocation; mandatory fresh snapshot on reconnect. |
| Runtime paths | Owner/mode/symlink checks, stale or corrupt manifest, simultaneous starts, crash during atomic writes, unrelated occupied port and authenticated incompatible build. |
| Lifecycle | Parent exit survival, no-client survival, Stop marker failure, launch/Stop/Resume races, crash/new epoch, signals and late blocking store settlement; no implicit replay or restart after Stop. |
| Load | Raw framing ambiguities, integer/depth limits, management reservations, chunked/oversized/slow bodies, header bounds, request/connection/subscriber caps, maximum-profile serialization, bounded auth/bootstrap traffic, responsive readiness/queries/drain under saturation. |
| Session models | Distinct user/profile namespaces with shared-port collision, logout and sleep/wake transitions; no cross-user attachment. Real signed sessions remain M11 integration evidence. |

M8's review distinguishes fakes, actual subprocess tests and signed macOS tests.
M9 receives the narrow development HTTP client, typed wire contract, launcher
and readiness interface, fresh-profile/credential namespaces, bootstrap
capability model and stop semantics. It extends the GUI to full API parity and
verifies signed fresh-credential behavior without legacy migration. M10 receives
the shared access gate and stable endpoint without an implemented `/mcp` adapter.
M11 receives the remaining signed session/upgrade/fast-user-switch matrix.
