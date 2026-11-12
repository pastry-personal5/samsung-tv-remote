# M8 wire decisions

Status: Request/response codecs, HTTP/SSE, executable bootstrap and native
client implemented for the isolated fake profile. Production cutover is M9.

The M6 contract still owns operation semantics. These decisions resolve its
representation gaps without changing device persistence or M7 admission.
`src/server/control_http/request.rs` and `wire.rs` implement the request side;
`tests/control_wire.rs` exercises every mutation shape. Actual loopback HTTP
fixtures now live in `tests/control_http.rs`; the executable path is exercised in `tests/server_process.rs`.

## IDs and integers

All UUID spellings are lowercase, hyphenated, exactly 36 bytes. Alternate
spellings fail. Epochs carry the complete runtime 128-bit value. Saved-device
UUIDs carry the complete device identity; on-disk `dev_<decimal>` keys remain
unchanged. Operation keys are client-created canonical UUIDs, not identities.

An operation/candidate UUID encodes 16 bytes as follows, in network byte order:

| Bytes | Value |
| --- | --- |
| 0–3 | `53 54 52 01` (`STR` and codec version 1) |
| 4 | Kind: 1 operation/observation, 2 candidate |
| 5 | Zero for operation; candidate index 0–31 |
| 6–7 | Zero, reserved |
| 8–15 | Full unsigned 64-bit operation counter; zero invalid |

These are opaque UUID-shaped identifiers, not randomly generated version-4
UUIDs. No epoch hash is squeezed into them. The enclosing `server_epoch` is
mandatory, including `?epoch=...` for ID lookup. Check supplied epoch against
current epoch before resolving a scoped ID, then check prefix, kind, reserved
bits, index and counter. The same counter may appear after restart; a scoped
UUID without its epoch is insufficient. Encoding never confers authorization.
Attempt and observation IDs refer to the same successful Probe operation;
Pairing presents both and they must agree. No transport ID map is needed.

Every `u64` counter/revision/generation is a JSON **string** containing canonical
unsigned decimal digits: `"0"`, `"9007199254740993"`, `"18446744073709551615"`.
Reject JSON numbers, signs, leading zeros, whitespace and overflow. This rule
also applies to future MCP DTOs. Display expiry is captured once as UTC Unix
milliseconds at terminalization, then formatted by the response adapter.
Monotonic time alone evicts records; lookup or duplicate replies cannot renew
retention. Candidate/observation display expiry is captured once at publication and does
not extend their independently checked monotonic authority.

## Strict requests

Decode directly into typed route DTOs with unknown-field rejection, including
common envelope fields. Duplicate fields, unknown enums, malformed JSON,
non-finite numbers and recursion beyond serde_json's default depth limit fail
before admission. Bodies have a 16 KiB limit checked before JSON parsing;
the transport must also enforce it while reading, independently of headers.
Maximum-field request fixtures cover the 256-byte Pair label input envelope,
full counters, fingerprints, IDs, recovery fields and both MAC interfaces.
Transport separately enforces header/framing/streaming limits; see the
[implementation record](milestone-08-implementation.md).

The common envelope is `server_epoch` and `operation_key`. No `client_id`
field is accepted. Route-specific fields are:

| Route | Additional fields |
| --- | --- |
| discoveries | None |
| probes | `host`, optional `candidate_id`; owner validates host/handle binding |
| pairings | Equal `attempt_id`/`observation_id`, `host`, equal full lowercase `fingerprint`/`confirm_fingerprint`, `label`, `selection_generation`, optional saved `device_id` and `recovery_transaction_id` |
| selection | `device_id`, `expected_generation` |
| connections | `device_id`, `selection_generation`, optional boolean `only_if_idle` (defaults false) |
| wakes | `device_id`, `selection_generation` |
| remote-actions | `device_id`, `selection_generation`, semantic `action` |
| devices/{id}/wake | Matching `device_id`, `selection_generation`, `expected_config_revision`, optional `wired_mac`, `wifi_mac`, `active_interface` (`Wired` or `WiFi`) |
| wakes/{id}/cancel | `device_id`, `selection_generation`; path is the named operation |
| devices/{id}/forget | Matching `device_id`, `expected_config_revision`, equal `confirm_device_id` |
| devices/{id}/recover-pairing or recover-forget | Matching `device_id`, exact `transaction_id`, equal `confirm_device_id` |
| server/stop | None; uses lifecycle capability rather than TV service |

Absent optional fields and explicit null both represent no value. Path/body
mismatch, malformed confirmations and invalid host/MAC syntax fail before
admission. Mutable policy, observation authority, configuration validity and
recovery transaction matching remain the owner's responsibility.

## Atomic private references and authorization

`ControlService::client_snapshot` and `subscribe` capture at most 64 newest
retained operation references for this identity in the same owner transition
as the shared snapshot. Shared events contain no private receipt/reference list.

The external access adapter holds credential digests and stable identities.
`submit_authorized` carries only a revocable, non-secret authorization lease.
The owner holds its read guard across its synchronous admission transition;
revocation obtains the write guard before returning. Thus work queued before
revocation cannot be newly admitted afterward. Already admitted work retains
its outcome. HTTP response and SSE delivery must additionally check revocation.

The owner captures `terminal_state` once at terminalization: selected saved ID,
`selection_generation` and the target's optional `config_revision`. All counters
remain decimal strings. Lookup never reconstructs these facts from newer state.
A shared snapshot exposes `admission_closed` independently of startup/storage
readiness, so the durable-Stop write can be visibly fenced before runtime drain.
Authenticated readiness additionally exposes the lifecycle state.


Terminal receipts use `state:"terminal"` and `result:{kind:...}`; pending uses
`state:"pending"` and null result, without guessed expiry. Stable
`expires_at_unix_ms` is a nonzero decimal string. Errors have `error:{code:...}`
and admitted failures retain operation metadata. The native projection checks
observation/attempt equality and candidate ownership/index before displaying
setup controls; the owner remains the final authority on handle use.

Readiness exposes `api_version`, `management_version`, build, epoch, serving
and lifecycle state. Bootstrap-only readiness additionally proves the bound
listener; see the [transcript review](milestone-08-bootstrap.md). Normal
readiness never returns a proof key or reusable credential.
