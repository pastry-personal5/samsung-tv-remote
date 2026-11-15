# P3-M1 private diagnostic contract

Status: Active — implementation contract; not a production Samsung schema.

The owner has requested P3-M1 implementation and confirmed a hands-on session
is available. This contract freezes the finite diagnostic slice before code.
It does not supply per-effect intent or accept a hardware result.

## Activation and ownership

The lane is compiled only with Cargo feature `p3-spike`. Default builds have
neither its endpoints nor its foreground controller. A feature-enabled signed
candidate still starts without a lease or any diagnostic TV request. The
foreground controller attaches through the existing signed local bootstrap as
GUI management principal and explicitly acquires a lease; attachment never
starts/resumes an owner, pairs, connects or clears Stop. An incompatible owner
requires a separately explicit all-client restart of the reviewed candidate.

Private authenticated routes are outside `/v1` and MCP. Opening a streaming
lease captures epoch, selected saved TV, selection generation and active session.
It requires ready paired/trusted ownership, no pending effects/storage, and one
available diagnostic port. Only one lease exists. The lease stream's drop sets
a shared cancellation flag outside request-queue capacity. The owner checks it
on its 100 ms tick; the session checks it before an unsent diagnostic write.
Release/controller loss aborts the session, fencing aliases, pending work and
uncorrelated reply families. Reconnection is always explicit. Reads stay usable;
ordinary TV mutations return busy during the lease. Revocation, Stop, expiry or
any captured identity fence revokes the lease and preserves terminal outcomes.

## Finite requests and limits

The only experiments are catalog, launch A/B, IME observation, HDMI 1/2/3,
tuner and Sources menu. Every experiment has a fresh UUID intent and captures
the lease identity. Each source/launch requires its own foreground confirmation.
An alias-setting command selects two distinct indices from this lease's validated
catalog and causes no TV effect. No external app ID is accepted. There is no
text/Enter/Power, deep-link content, arbitrary key or generic JSON sender.

Bounds: one outstanding experiment; diagnostic command queue 1; event queue 16;
64 retained intents per lease; 64 apps; app IDs 128 ASCII-safe bytes; display
names 128 bytes after control/bidi stripping; 16 KiB inbound frame/message;
4-second write deadline; 10-second catalog/launch/source windows; 30-second
IME window; 30-minute lease. A hit is inconclusive and ends that lane rather
than raising a production cap. Each request is written at most once. Duplicate
intent keys return the retained result or conflict, never a second effect.

Catalog and launch wire responses have no assumed echoed request ID. Matching
frames inside a single outstanding experiment are marked ambiguous association,
not proven replies or automatic capability facts. Catalog timeout quarantines
catalog acquisition until explicit reconnect; launch timeout quarantines launch.
Unknown/malformed events never become unsupported. No result triggers a retry.

## Candidate encoders and observations

Research hypotheses follow the primary [remote implementation](https://github.com/xchwarze/samsung-tv-ws-api/blob/master/samsungtvws/remote.py)
and [event parser](https://github.com/xchwarze/samsung-tv-ws-api/blob/master/samsungtvws/event.py):
`ed.installedApp.get`, `ed.apps.launch` with ordinary `NATIVE_LAUNCH` and no
content parameters, and `ms.remote.imeStart`/`ms.remote.imeEnd`. The candidate
catalog envelope is `data.data` as an array. Alternative/unreviewed shapes are
counted as inconclusive. These hypotheses do not freeze product schemas.

The session retains validated IDs only in memory and exports no ID, token,
pin, address, field identifier, typed text, Base64 or raw frame. Debug/errors
are finite/redacted. Local catalog choice names are bounded display data;
committable report/fixtures substitute synthetic names and IDs. Unknown events
produce counts only. IME extraction retains only event/type structure and
lifecycle counts; payload strings are discarded before crossing the session
port. Reports separate machine counts/write outcomes from operator observations.

The test gate covers cancellation, controller death, competing leases, guarded
admission, duplicate intent, timeout quarantine, malformed/oversized/duplicate
frames, sensitive markers and exact fake effect counts. The live spike starts
only after those checks and a signed candidate exist.

## Implemented private routes

- `POST /_p3/lease`: exact epoch, saved device ID and selection generation;
  returns the held lease stream. No TV effect.
- `POST /_p3/intent`: lease, UUID intent, finite experiment and matching
  `confirm_experiment`; never accepts a frame or external app ID.
- `POST /_p3/aliases`: lease plus two returned numeric catalog choices;
  only memory changes. Launch receipts bind the current alias revision.
- `GET /_p3/status?lease=UUID`: bounded retained machine observations.

All routes require the existing management authority, including status. Default
builds have no such dispatch. Stream drop revokes a flag outside queue capacity;
status-triggered cleanup publishes the disconnect atomically, as does timer
cleanup. Closed-lease reports retain terminal uncertainty; aliases are cleared.
Opening another lease requires explicit connection recovery, not a hidden retry.
