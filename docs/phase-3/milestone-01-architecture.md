# P3-M1 Architecture: Capability Evidence

Status: Active — semantic foundation implemented under the
[offline implementation authorization](milestone-01-overview.md#implementation-authorization--2026-11-14); Samsung wire schemas remain gated.

## Current seams and ownership

The existing `application/runtime.rs` owner publishes `Snapshot` and `Event`,
owns the selected TV and forwards session events tagged with selection and
session generations. `application/tv_session.rs` exposes write, disconnect,
token-rejection and content-free semantic IME lifecycle events. `infrastructure/samsung/codec.rs` parses
authorization events; the active loop in `samsung/session.rs` currently ignores
other Samsung events. There is no installed-app or IME parser to reuse.

Extend those seams. Domain values describe semantic capability and evidence;
the application decides whether evidence is current and what it permits.
Samsung event names, JSON shapes and text encoding remain in infrastructure.
The runtime alone updates facts. HTTP, MCP and native presentation consume
projections and never open TV connections or interpret Samsung frames.

## Proposed model

Use finite typed values rather than free-form feature names or a boolean
`supports_everything`. Names below describe the contract, not a frozen Rust or
JSON schema. Freeze field spelling after the spike.

| Value | Meaning |
| --- | --- |
| Capability key | Installed-app discovery, ordinary app launch, native IME events, or one individual source key. Discovery does not imply launch. |
| Support | `unknown`, `supported`, `unsupported`; only positive evidence or an unambiguous feature-specific negative response can leave unknown automatically. |
| Evidence source | `none`, `tv_response`, `tv_event`, `user_confirmation`. Research, a write receipt and a model-name match are not evidence sources. |
| Observation | Saved device ID, evidence source and observation time; optional bounded firmware clue with provenance. No raw frame or content. |
| Scope | Session evidence binds to epoch, saved TV, selection generation and connection/session generation. Future durable evidence additionally binds to the saved TV's trust/pairing lineage. |
| Freshness | `current`, `stale`, or `unobserved`; stale evidence can be displayed as history but cannot grant availability. |
| Availability reason | Application-derived reason such as evidence missing/stale, not selected, disconnected, recovery blocked or feature unsupported. It is separate from support. |

An unknown/unobserved fact uses source `none` and no observation time. A stale
supported observation remains historical evidence, not current permission.
Expose one bounded latest observation per key, not an unbounded event history.
A live IME-active flag belongs to the current session; IME support and current
IME activity are different facts. Duplicate starts are idempotent; an end with
no current start cannot invent a session. M4 will define sendable session IDs.

Source-key writes never set support. In M2 the user can confirm or hide each
key for this saved TV; that intent must use existing mutation guards and config
revision semantics. M1 defines this evidence source but adds no confirmation
mutation or label persistence. Hardware observations in a committed evidence
record are not automatically imported as facts for every user's saved TV.

Launch support needs a feature-specific response or explicit human observation.
Neither an installed-app reply nor launch silence proves it. A failure for one
app must not mark all launching unsupported. Recognized errors without clear
feature attribution leave support unknown. Unknown or malformed frames cannot
overwrite a valid observation or turn into negative evidence.

## Acquisition and bounded parsing

The spike is an explicit diagnostic experiment, separate from product reads.
The [automation contract](milestone-01-spike-automation.md) defines its missing
owner harness, lease, finite intents, bounds and evidence outputs. Candidate
diagnostic wire shapes may precede evidence; they do not freeze production
schemas or become a generic frame-sending interface.
After schema freeze, M1 production acquisition is passive: consume recognized
events on the existing authorized session. Installed-app and launch replies
are interpreted only when correlated with an outstanding typed request in the
same session. Do not assume the TV echoes a request ID: serialize each
uncorrelated request family, quarantine it after a timeout until reconnect,
and reject unsolicited/late replies as request evidence. Record the actual
association method during the spike. Single-flight is not proof of attribution:
if an unsolicited or duplicate event is indistinguishable from a reply, retain
it only as an ambiguous diagnostic observation. Do not grant request-derived
support from it. Slice 3 must settle an evidence-backed correlation policy or
leave that fact unknown. M3 owns the production catalog request/refresh
trigger; M1 does not send it on every Connect, snapshot read, subscription or `inspect_tv` call.
Until a valid observation exists, the corresponding fact remains unknown.

Extend the codec with evidence-backed named event variants and translate them
to semantic `TvSessionEvent` values. Preserve authorization and token-rejection
behavior. Apply limits before JSON allocation and before decoding nested
payloads. The current 16 KiB event cap remains until the spike demonstrates a
need to change it; an oversized valid catalog is an explicit unresolved limit,
not permission for unbounded parsing or truncation into a successful reply.

Freeze limits for nested payload bytes, app count, ID/name bytes, firmware
bytes, event queue capacity and request deadlines in slice 3. Record measured
values and chosen headroom. Bound both wire and decoded sizes; validate the
whole reply before publishing evidence. Strip controls and bidi-override
characters from display data. Preserve ID integrity: reject an unsafe ID
instead of transforming it into a different launch target. Reject duplicate
IDs or sanitized collisions where identity would become ambiguous. M3 owns
the resulting usable catalog, not M1's capability projection.

IME parsing extracts only the lifecycle signal. Never forward field contents,
typed text, Base64 or raw JSON through application events, debug formatting,
receipts or diagnostics. Unknown bounded frames are ignored. Malformed input
cannot update evidence; oversized input follows the existing bounded transport
failure policy. Keep queues bounded; if lifecycle events cannot be delivered,
invalidate the session rather than dropping an IME-end event and leaving an
apparently active field.

## Invalidation and time

Retain the runtime's generation/session envelope and check it before applying
every new observation. Capture target identity when starting a request, never
resolve it from the selected TV when its reply arrives. Reject late completion
after a fence even if the user selects the same TV again.

| Transition | Required result |
| --- | --- |
| Disconnect, reconnect/session replacement, token rejection | Clear active IME; prior session observations become stale. New session starts unobserved for action gating. |
| Selection change | Invalidate outgoing session facts before publication; never copy them to the new selection. Returning to the same TV requires fresh session evidence. |
| Re-pair or certificate/trust change | Invalidate facts tied to the old lineage before any new connection can grant availability. Failed certificate checks cannot establish new evidence. |
| Forget or Forget recovery | Clear in-memory facts and reject late frames. No residual per-TV facts survive successful removal. |
| Server restart | New epoch, no session evidence or active IME. Never restore a live field from disk. |
| Expiry or reported firmware change | Retained evidence becomes stale; refresh requires the documented feature policy, never a hidden read-side effect. |

Use an injectable clock: monotonic elapsed time for live deadlines/expiry,
wall time only for display/persistence timestamps. Publish a new snapshot
revision when a current fact expires so SSE observers cannot retain current
availability indefinitely. A backward wall-clock change must not extend a
deadline; future-dated durable data is stale. Session facts have no validity
beyond their session. Durable TTLs belong to the milestone introducing that
data (notably M3's catalog) and must be recorded before implementation.

Retain one bounded historical observation per key/TV plus a separate live
IME-active value; a new unobserved session does not overwrite historical
support with unsupported. Invalidation is published atomically with the
connection/selection transition, so no snapshot pairs a new session with old
current facts. Duplicate identical observations should not churn revisions.

M1 keeps observations in memory and changes no saved-device/trust disk format.
Its staleness tests include invalidation and a fake expiring evidence policy;
they do not claim a persistent catalog exists. When M2/M3 introduce durable
facts, place them in the existing per-TV record transaction or extend the
Forget journal with tested cleanup stages. Require backup/recovery and format
migration tests before shipping. A sidecar without Forget recovery is invalid.

## HTTP, events and client projections

Add a finite capability summary per saved TV to the application's snapshot,
then project the same summary into existing `/v1` snapshot/detail responses
and SSE snapshots. Reuse the existing epoch, revision and atomic subscription
contract; capability changes are ordinary state revisions, not a second event
stream. Include the TV identity and evidence context so historical/non-selected
facts cannot look current. Recovery-only targets have no invented facts.

Extend `control_client/projection.rs` and typed client values together. Missing
capabilities from a compatible older response decode as unknown, never
supported; invalid current facts fail closed. Preserve the existing build/
protocol compatibility fence. New timestamps and counters use the existing
lossless decimal-string counter convention. Cap the number of feature entries
and measure worst-case snapshot/event size for the existing 1,024-TV profile
against HTTP/client output limits; avoid attaching full app lists or frames.

Extend only MCP `inspect_tv` with the inspected saved TV record's compact capability
summary (including stale/non-selected reasons). Keep tool definitions stable
across connection state. Do not expand `list_tvs` with verbose evidence or add
a refresh/probe tool. Update result schemas/corpus and measure the complete
catalog, ordinary inspection result and relevant workflow in the accepted
installed-client budget lane. Reusing a tool does not exempt result growth
from that gate. MCP strings remain bounded data, never instructions.

No client read triggers a probe, reconnect, launch, source change or text send.
Unknown discovery support must still allow an explicitly admitted M3 catalog
request on a ready connection; otherwise a new TV could never establish its
first positive observation. M2 user-configured source keys use user intent,
not a nonexistent source-state probe. M4 requires a current field/session.
Define availability per operation; do not apply a universal supported-only
rule to acquisition.

Availability is advisory for future UI gating; each mutation still checks the
current epoch, target, generation, operation key, session and applicable config
revision inside application admission. `not_sent`, `written`, and `uncertain`
retain their existing meaning and no uncertain effect is replayed.

## Verification and implementation map

| Layer / likely files | Deterministic proof |
| --- | --- |
| New domain capability module; application capability policy | Evidence/source invariants, unknown versus unsupported, per-key/per-TV isolation, fake-clock expiry and backward-time behavior. |
| `samsung/codec.rs`, `samsung/session.rs`, `tv_session.rs` | Recorded sanitized fixtures plus synthetic malformed, oversized, unknown, nested, duplicate and sensitive-IME cases; authorization regression coverage. |
| `runtime.rs`, `runtime_connection.rs`, setup/Forget paths | Old-session and old-selection frames rejected; same-TV reselection, re-pair, token rejection, certificate mismatch, restart and Forget cannot revive facts; queue-pressure invalidation. |
| HTTP response and typed client projections | Round trip, old-compatible response fallback, malformed evidence rejection, lossless times/counters, maximum-profile bounds and SSE resubscription. |
| MCP `tools.rs`, result schema and corpus | `inspect_tv` matches HTTP at one revision; no new tool; two readers cause zero TV effects; measured catalog/result/workflow budget. |
| Existing shared HTTP/MCP fake-runtime harness | Simultaneous clients observe the same updates and invalidation, late replies never retarget, read/reconnect recovery never replays a mutation. |

Use existing integration tests and fake gateway/session boundaries; add focused
capability tests under `tests/`. Use fake clocks and explicit barriers rather
than wall-clock sleeps. Do not claim synthetic fixtures establish hardware
compatibility. Follow the overview's completion checklist and the
[contribution-guide validation](../contribution-guide.md#required-validation).

## Implemented foundation — 2026-11-14

`domain/capability.rs` defines the finite semantic set. The application’s
`capabilities.rs` retains one observation per TV/key, injects monotonic time
at the policy boundary, and separates live IME activity from historical
support. `runtime.rs` synchronizes the evidence session and removes forgotten
records before publishing a transition. Identical IME observations do not
publish revisions. The semantic session port has content-free IME start/end
variants for fakes and future reviewed adapters. The Samsung adapter emits
neither variant yet; production facts remain unknown.

HTTP snapshot, SSE snapshot, device detail and MCP `inspect_tv` share
`response::device_detail`. `list_tvs` remains compact. The native projection
accepts an absent capability field as unknown and rejects contradictory
current evidence. Each wire summary contains exactly eight ordered facts,
a saved-TV ID, context reason and live-IME flag. In-memory history belongs to
one owner epoch; `evidence_epoch` encodes it once per TV, while individual
observations carry lossless selection/session counters. No firmware clue is
invented or parsed; evidence-backed firmware handling remains open.

This is an implementation-stage semantic projection, not a frozen Samsung
schema or an accepted MCP budget. See the [implementation record](milestone-01-implementation.md)
for measured bounds, validation and remaining acceptance.
