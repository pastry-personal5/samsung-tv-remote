# P2-M10 implementation contract

Status: Active. Initial transport/service freeze and enrollment controller;
connection workflow and installed acceptance remain incomplete.

The owner's instruction to complete M10 permits development ahead of M9's
native acceptance, as recorded in the [phase plan](phase-2.md). The
[architecture](milestone-10-architecture.md) still owns the complete milestone.
This record freezes implemented interfaces and explicitly identifies remaining
slice-1 work; it does not replace the completion checklist.

## Transport

Use only revision `2026-07-28` on the existing authenticated literal-loopback
listener. No initialization session, GET stream, stdio server, task machinery
or subscription capability is advertised. `server/discover` and `tools/list`
have private catalog TTL 300,000 ms; HTTP responses use `no-store`. Discovery
identifies the actual executable build. A first `tools/call` is independent of
discovery and earlier connections.

Validation order is shared Host/Origin/authentication, HTTP framing/method/media,
bounded unique-member JSON, envelope/ID, body metadata, mirrored headers,
supported revision, capabilities/client-info, method/parameter shape and tool
name, semantic capacity, argument schema/typed decoding, service admission.
Header/body version disagreement precedes unsupported-version handling. Missing
body version/capabilities yields invalid params. Bounded unknown `_meta` keys
are retained as data; they never change authority.

| Condition | HTTP | JSON-RPC response |
| --- | --- | --- |
| Missing/invalid bearer or forbidden Origin/Host | Existing shared 401/403/429 policy | No parsed request ID; no tool/service admission. |
| GET/DELETE or another non-POST `/mcp` | 405, `Allow: POST` | Invalid request; no legacy stream/session. |
| Required Accept types unavailable | 406 | Invalid request. |
| Invalid content encoding/type, oversized body | Existing 415/413 policy | Rejected before decoding/admission. |
| Invalid JSON syntax | 400 | `-32700`, ID omitted. |
| Duplicate members, batch/response body, missing/null/fractional ID | 400 | `-32600`; preserve an unambiguous valid ID when available. |
| Missing metadata, invalid capabilities or method parameters | 400 | `-32602`, parsed ID. |
| Missing/malformed/mismatched MCP headers | 400 | `-32020`, parsed ID. |
| Matching unsupported header/body version | 400 | `-32022`, `data.requested` and `data.supported`, parsed ID. |
| Unknown RPC method | 404 | `-32601`, parsed ID. |
| Unknown tool | 400 | `-32602`, parsed ID. |
| Semantic request capacity exhausted after parsing | 429 | Application code `1001`, parsed ID; no admission. |
| Tool argument or service/policy failure | 200 | Complete tool result, `isError: true`, stable error and next step. Retained failures preserve their operation reference. |
| Enclosing request deadline after parsing | 503 | `-32603`, parsed ID and original-key recovery guidance; no claim of non-admission. |

The [protocol fixtures](../../tests/mcp_protocol.rs) and
[shared-listener fixtures](../../tests/support/mcp_http.rs) own these cases.
The [MCP schema](https://modelcontextprotocol.io/specification/2026-07-28/schema)
is the normative reference for discovery, request metadata and result framing.
Application backpressure/media choices above are local policy, not additional
normative MCP error codes.

## Shared capacity and deadlines

| Reservation | Global | Per principal |
| --- | ---: | ---: |
| Ordinary execution | 48 | 8 |
| Unclassified MCP POST parsing | 4 | 2 |
| Queries | 8 | 4 |
| GUI management | 4 | 2 |
| Total request ceiling | 64 | 16 |

Classification is carved from M8's ordinary allowance: ordinary execution plus
classification remains 52 global/10 per principal. `/v1` mutations use execution
slots; `/v1` reads and MCP reads share query slots. Release classification
before taking a semantic slot, with no await or nested permit acquisition.
Saturation rejects promptly. Every acquired reservation remains held through
its response flush/drop, including invalid classified requests. Connections
remain bounded at 128. MCP has no management tools.

Whole request bodies remain at most 16 KiB including metadata/envelope;
metadata is at most 4 KiB. Header/body-read and outer HTTP deadlines remain
M8's existing limits. MCP execution uses a deadline 14 seconds from dispatch,
inside the shared 15-second outer handler deadline. Lookup wait is 0–10 seconds,
capped by that original deadline minus 250 ms for encoding. At most four
global/two per-principal waiters consume subsets of the existing query slots
and existing owner subscriber limits. Subscription closure performs one final
private lookup; pending work returns service-unavailable with its receipt.
Disconnecting the read drops its subscription, never the admitted TV effect.
MCP output is capped at 128 KiB; oversize results are RPC errors with the
original ID and recovery guidance.

## Tools and projections

The sorted, state-independent catalog lives in
[`catalog.rs`](../../src/server/mcp/catalog.rs). Its input schemas are strict
root objects. All counters use canonical decimal strings; UUID codecs and
typed command validation are shared with `/v1`. Read-only/idempotent hints are
true only for the three reads; mutations explicitly set both false.

| Tool | Authoritative command/query |
| --- | --- |
| `cancel_wake` | `MutationRoute::CancelWake` → `Command::CancelWake`; explicit selected target/generation and named owned operation. |
| `connect_tv` | `MutationRoute::Connect` → `Command::Connect`; explicit selected target/generation. |
| `discover_tvs` | `MutationRoute::Discover` → `Command::Discover`. |
| `forget_tv` | `MutationRoute::Forget` → `Command::Forget`; matching target confirmation/config revision. |
| `get_operation` | Exactly one `ControlService::lookup` or `operation`; private principal/epoch scope. |
| `inspect_tv` | One atomic `ControlService::snapshot`; exact target/config/recovery guards, including orphan cleanup targets. |
| `list_tvs` | One atomic `ControlService::snapshot`; complete paginated enumeration and separate selected context. |
| `pair_tv` | `MutationRoute::Pair` → `Command::Pair`; equal observation/attempt, full matching certificate confirmation, host/generation and explicit recovery variant. |
| `probe_tv` | `MutationRoute::Probe` → `Command::Probe`/`ProbeCandidate`; optional private expiring candidate. |
| `recover_forget` | `MutationRoute::RecoverForget` → `Command::RecoverForget`; exact target/transaction/confirmation. |
| `recover_pairing` | `MutationRoute::RecoverPairing` → `Command::RecoverPairing`; first-Pair compensation only. |
| `remote_action` | `MutationRoute::Remote` → finite semantic `Command::Remote`; selected target/generation. |
| `select_tv` | `MutationRoute::Select` → `Command::Select`; compare-and-swap expected generation. |
| `set_wake` | `MutationRoute::SetWake` → `Command::SetWake`; selected target/generation/config revision. |
| `wake_tv` | `MutationRoute::Wake` → `Command::Wake`; captured configuration, one packet maximum. |

Every mutation is decoded by the existing strict shared route DTOs, then
submitted with the authenticated admission lease. Original keys are retained
privately with receipts and never exposed by shared events. Typed command
equality owns deduplication; RPC IDs, field order and metadata do not participate.

Output schemas live in [`results.rs`](../../src/server/mcp/results.rs). Keep
structured output plus identical serialized text until actual client evidence
justifies an alternative. These duplicated channels must both be measured.
Terminal write/uncertainty and Wake packet/readiness remain the shared facts.
No adapter owns a TV socket or storage policy.

Page size defaults to four, maximum 16. List cursors bind principal, epoch,
query, size and a digest of sorted saved configuration; connection-only global
revisions do not stale a page. Operation pages bind the lookup selector and
retained outcome. Cursors are HMAC-authenticated, at most 1,024 characters,
expire after 120 seconds on a monotonic clock, and store no server-side page
registry. Paging never renews candidate/observation authority. Labels/name/model
previews retain at most 80 Unicode characters with explicit truncation; hosts,
IDs, fingerprints and cleanup guards remain complete. Summaries include startup,
admission and recovery targets even with no saved rows. Inspecting an orphan
recovery target returns `tv: null` plus its exact transaction. Readiness on an
unselected target is explicitly `not_selected`, not another TV's readiness.

## Open delivery work

No installed MCP endpoint/helper or model-visible budget result is accepted
from the initial code/fixtures. Claude Code 2.1.322 and user-scope JSON writer
support were observed in isolated configuration; runtime/session selection and
helper execution still require a real endpoint test. Initial mutation calls
return immediate receipts; a bounded observation fast path, frozen corpus
projection/call counts and actual schema conformance coverage remain to finish.

The initial enrollment controller persists disabled/enabled state independently
of API-item presence. Fresh and existing profiles default disabled. Live
management is GUI-only `GET /v1/mcp` and `POST /v1/mcp/enrollment`; changes carry
current epoch, one UUID operation key, expected enrollment generation and an
enabled flag. Matching retained last-key intent recovers its result; changed
intent conflicts and stale generations cannot regrant access. Routine identical
Connect preserves its bearer. A new grant derives the MCP bearer with HMAC-SHA256
over a versioned domain, profile, generation and fresh nonce using the protected
MCP API item. Only the derived bearer reaches bootstrap clients. Disconnect
fences leases before durable disable; failed persistence remains fenced in the
current owner and cannot be undone by bootstrap refresh. Regrant keeps the
same principal/retained operation registry but changes its bearer. No nonce,
base secret or derived bearer appears in management responses.

| Enrollment/configuration transition | Required order and recovery |
| --- | --- |
| Fresh/existing profile without enrollment | Disabled generation zero regardless of a readable MCP API item; no bootstrap grant. |
| Fresh Connect/regrant | Validate client/configuration and save the intended token-free entry first. Then compare enrollment generation, derive the next bearer, fence the old lease, persist the new generation/nonce, publish access and acknowledge. A configuration failure must not invoke grant. |
| Credential failure before grant | Preserve prior durable state; show saved-but-unverified configuration when applicable. Do not recreate a blocked/missing established API item. |
| Lost grant reply | Recheck/recover the same generation/key. A matching last-key payload returns its durable record; a changed payload conflicts. A live fenced grant needs explicit repair rather than silent refresh. |
| Disconnect | Fence active MCP admission, persist disabled state, then remove only the unchanged app configuration entry. A failed persistence write reports failure and stays fenced in that owner. Entry removal requires durable-disable evidence. |
| Lost Disconnect reply | Query management status and the original last operation key before removal; report incomplete revocation when it cannot be established. Neither helper execution nor recheck can enable access. |
| Failed/conflicting entry removal | Keep enrollment disabled and retain a safe removal retry. Never remove another writer's changed entry. |
| Explicit Connect after disable | New key, expected current generation and fresh nonce/bearer under the same principal; old bearers remain invalid and retained TV results keep their private owner. |
| Crash/restart | Honor the readable durable record; missing established/corrupt metadata fails closed. No existing config or Keychain presence promotes disabled state to enabled. A published grant before live lease publication is recovered by the next owner from that record. |

The controller implements the enrollment side of this table. The connection
coordinator now implements ordered configuration publication/removal against
the live/offline management port, with exact-entry checks and a whole-workflow
advisory lock. An already identical desired entry is a byte-preserving no-op,
including a retry whose original expected entry was absence; enrollment still
checks the original generation/key. Different entries require an exact expected
match before replacement or removal. Blocking edits retain that lock across
cancellation. Isolated
tests cover saved-but-ungranted configuration, failed removal after revocation,
stale generations, original-key recovery, idempotent Connect/removal and
relocation. GUI/client capability and scope preflight, installed lock
interoperability, complete crash coverage and actual-client acceptance remain
delivery work.

Blocking enrollment transitions retain a duplicate of the lifetime owner lock,
so HTTP cancellation/shutdown cannot overlap unfinished persistence with a new
owner. Offline dispatch and stopped-profile endpoint preparation are implemented
under ownership without constructing TV adapters. Before accepting slice 4,
complete the enrollment/config/grant transition table, destination-bound signed
helper, protected offline mode, conflict-safe config writer and GUI action.
Before installed acceptance, prepare the signed isolated corpus harness,
disposable-account production helper smoke and separate-host permission lane
from the [acceptance topology](milestone-10-architecture.md#acceptance-topology).
These dependencies remain open; the complete milestone cannot be marked Done.
