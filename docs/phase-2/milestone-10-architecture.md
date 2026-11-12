# P2-M10: Streamable HTTP MCP

Status: Planned

## Baseline and entry contract

M7 supplies the headless service. M8's [completion review](milestone-08-review.md)
and [wire decisions](milestone-08-wire.md) identify implemented shared access,
HTTP, lifecycle and client modules. `server/mod.rs` has no MCP adapter. M9's
GUI/signed production cutover remains a separate gate; the fake development
profile cannot be promoted to production.

Before implementation record the accepted shared command/query interface,
wire codecs, principal/capability mapping, private lookup, admission/rotation
ordering, limits, endpoint ownership and protected attach transcript. Use
M8's transport and lifecycle infrastructure, not another listener, operation
registry or launcher. Signed acceptance depends on M9's chosen executable
identity, fresh profile and credential verification.

## Approach

Place MCP transport and tool definitions under `src/server/mcp/` inside the
control server process. The adapter validates JSON-RPC, MCP transport headers,
protocol version, client credential, Origin, tool names, and typed arguments.
It translates each tool call to the same application service command or query
used by the control HTTP adapter. The server-wide selected TV and generation
remain authoritative across GUI and MCP clients.

Implement only `2026-07-28` initially, matching M6's supported revision set.
Recheck the installed CLI before freezing fixtures. Prior fake-endpoint
health checks do not prove installed tool calls or model workflows.
The selected [Streamable HTTP
revision](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
must be implemented as specified for that revision. The current 2026-07-28
revision uses POST and no longer defines the earlier GET stream or
protocol-level sessions; support for an older Claude Code revision requires
an explicit compatibility decision and separate versioned tests. For the
2026 revision, validate
`MCP-Protocol-Version`, `Mcp-Method`, and applicable `Mcp-Name` against the
JSON-RPC body and reject mismatches as specified. Return JSON-RPC JSON or
request-scoped SSE as allowed by the selected revision; no shell command or
stdio process stands in for the HTTP endpoint.

Use short JSON tool responses for mutating calls: admit the operation and
return its ID. A separate lookup tool obtains later status. This avoids
coupling a long-running TV effect to an MCP SSE response whose close means
request cancellation. Require the server epoch and client operation key on
mutations; same-key calls with changed arguments are rejected. A probe result
gives a bounded observation ID and safe certificate fingerprint. The Pairing
confirmation tool includes that observation and matching fingerprint; the TV
still asks its owner to approve. Forgetting a saved TV uses an explicit
`confirm_device_id` equal to the requested saved device ID. This is a
typed confirmation field, not a server-issued approval token. These checks validate an explicit
second step but cannot establish that a human approved an agent's call. The
adapter never receives the stored TV pairing token.

Keep the tool inventory small and fixed for a given protocol/authorization
version. One semantic remote-action tool takes a validated action enum and
maps Power Toggle to its distinct service path; never expose each remote
button as a separate tool. Do not hide Pairing or forget inside a generic
executor or multi-command batch. Write concise, front-loaded tool descriptions
and one short server instruction that helps tool search find TV setup and
control without repeating a long policy block in every tool. Keep the
relevant safety rule beside each sensitive argument. Advertise truthful
read-only/destructive annotations, with server authorization and confirmation
checks unchanged. In the 2026 revision, serve `tools/list` in deterministic
order with private caching hints suited to a stable catalog; TV state changes
do not change the tool list. Older negotiated revisions follow their own
list/cache rules. Verify Claude Code both with deferred tool search and with
all definitions loaded upfront.

Make read results summary-first and bounded, with a separate inspect path
for detail; discovery candidates use opaque IDs and reveal only data needed
for the next safe step. Mutations return a small operation receipt rather
than the entire server snapshot, and lookup returns the typed terminal fact
without repeating the original request or a protocol trace. A bounded wait
parameter on read-only operation lookup may replace rapid repeated polling;
its cancellation cannot affect the operation. Error results use a stable code
and one actionable next step, with minimal fresh target context where that
avoids another query. Test whether the target client consumes `content`,
`structuredContent`, or both before choosing a result shape: the newer spec
recommends a serialized text copy of structured data for compatibility, so
the duplicate may increase model-visible tokens. When an `outputSchema` is
declared, structured results must conform to it. Measure these choices against
the M6 corpus, including extra calls induced by over-terse outputs. Never
shorten a fingerprint, precondition, or `uncertain` result for a token target.

Use a fake application service for protocol conformance and security cases,
then a fake TV boundary for end-to-end GUI/MCP concurrency. Verify the real
Claude Code [HTTP MCP connection](https://code.claude.com/docs/en/mcp-quickstart)
with a user-scoped, token-free configuration. Its supported
[`headersHelper`](https://code.claude.com/docs/en/mcp#use-dynamic-headers-for-custom-authentication)
invokes a bundled signed helper to start or attach to the HTTP server and
emit only the MCP authorization header. This is not an MCP stdio server.
The helper must reject intentional Stop and distinguish a hostile/stale port
from this app's authenticated readiness. Claude Code can rerun the helper and
retry once on `401`/`403`, so authorization occurs before side-effect
admission and duplicate operation keys remain safe within retention. Test
without publishing a real token or local network address, and preserve
unrelated Claude Code configuration. Do not assume Claude Desktop supports
this helper.

## Module and composition boundaries

| Proposed boundary | Responsibility |
| --- | --- |
| `src/server/mcp/mod.rs`, `protocol.rs` | Route `/mcp`, validate envelopes, dispatch discovery/list/call and map protocol errors. |
| `src/server/mcp/catalog.rs` | Ordered tool definitions, input/output schemas, annotations and shared instruction. |
| `src/server/mcp/tools.rs` | Decode typed arguments and call the shared service with authenticated client context. |
| `src/server/mcp/results.rs` | Safe bounded summaries, receipts, outcomes and actionable errors. |
| Shared M8 access/wire/lifecycle modules | Authentication, authorization, ID/counter encoding, endpoint protection and launch/attach. |
| Executable helper composition | One-shot signed auth-header mode dispatched before Iced or production adapters. |
| Migrated GUI integration | Explicit Connect Claude Code action and visible configuration/setup failures. |

Exact file splits follow the accepted M8 handoff. The adapter calls the shared
application service directly; it does not loop back through `/v1`, own a
second policy path or read pairing credentials. The helper uses protected
bootstrap and obtains only a client credential. GUI mode remains free of
production TV/storage adapters after M9.

## Protocol fixtures and resource limits

Implement `server/discover`, `tools/list` and `tools/call`; advertise only
implemented capabilities. Start with JSON responses and no extra SSE,
subscription, resource, prompt, elicitation or task machinery. Discovery
advertises the supported revision, tools capability and brief instruction.
There is no `initialize` fallback.

Before coding, freeze a fixture table of exact HTTP status, JSON-RPC code and
response-ID behavior from the selected transport,
[versioning](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
and [tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)
specifications. Cover methods/media types, malformed JSON, batch bodies,
invalid envelopes/IDs, unknown methods/tools, invalid arguments and unsupported
versions. Include missing, conflicting and encoded metadata headers. Accepted
notifications have no response body. Distinguish protocol errors from tool
execution failures; a transport error cannot establish that an effect was
not sent.

Host/Origin and authentication checks precede protocol dispatch; authorization
is rechecked at admission. Reuse M8 connection/body/time limits. Explicitly
budget JSON-RPC envelope overhead and reserve capacity for read-only recovery.
Bound concurrent lookup waiters so they cannot occupy all request slots.
Reject oversized, duplicate and unknown argument fields without admission.

## Catalog and schema freeze

M6 began with 13 semantic tools. M8's reconciled corpus proposes 15 by naming
first-Pair cleanup and Forget cleanup separately, preserving their distinct
transaction/confirmation guards without conditional destructive overloads.
Freeze M10's final catalog in deterministic order and calibrate its token cost.
Check input/output schemas
against typed decoders/projections with fixtures. Annotations describe actual
behavior; they confer neither permission nor an automatic-retry guarantee.
Only list, inspect and lookup are read-only. Probe/discovery can open network
connections. Forget is destructive; evaluate other annotations from their
service effects rather than their names.

| Tool | Shared service behavior and guards |
| --- | --- |
| `list_tvs` | Saved-TV summary, selected target, current epoch/generation; no TV network access. |
| `inspect_tv` | One target's configuration, readiness and recovery facts; no physical-state inference. |
| `discover_tvs` | Bounded discovery with client-bound expiring candidates. |
| `probe_tv` | Validated host with optional bound candidate handle; fresh certificate observation. |
| `pair_tv` | First Pair/re-pair with observed target, full matching pin, generation and explicit recovery variant where applicable. |
| `select_tv` | Explicit server-wide selection with expected generation; retain same-ID generation increment and session fencing. |
| `connect_tv` | Connect selected saved target under observed generation. |
| `set_wake` | Selected target/generation and expected config revision; validated MAC/interface fields. |
| `remote_action` | One finite semantic action under selected target/generation. |
| `wake_tv` | One captured-config Wake operation under selected target/generation. |
| `cancel_wake` | Named owned Wake operation; cannot recall a packet already sent. |
| `get_operation` | Private lookup by exactly one ID or original key, epoch and optional wait of 0–10 seconds. |
| `forget_tv` | Ordinary removal with exact target confirmation and config revision. |
| `recover_pairing` | Exact target/transaction/confirmation; compensates interrupted first Pair only. |
| `recover_forget` | Exact target/transaction/confirmation; retries only the marker's remaining cleanup. |

Every mutation, including discovery, probe and cancellation, requires a current
epoch and a UUID key allocated once per intent. Reuse stable authenticated
identity across credential refresh and private lookup. Full client capabilities
must reach every supported setup/recovery action without gaining lifecycle
management permission.

The corpus remains a proposed MCP projection, not an installed adapter. M8's
schema reconciliation is now executable in `tests/control_documentation.rs`:

- Pair uses host, equal attempt/observation IDs, `fingerprint` and matching
  `confirm_fingerprint`, optional saved `device_id` and
  `recovery_transaction_id`. Never substitute a newer certificate.
- Probe carries the validated host plus optional client-bound `candidate_id`.
- Wake names `wired_mac`, `wifi_mac`, `active_interface`; cancel names selected
  device/generation and the exact operation. Every u64 counter is a canonical
  decimal string. Scoped handles always travel with the full server epoch.
- First-Pair compensation and Forget compensation are separate commands/tools
  with exact target, transaction and confirmation. Re-pair recovery instead
  obtains fresh observation/approval using `pair_tv` and its recovery ID.
- Private lookup and missing/expired/new-epoch uncertainty follow the shared
  runtime. M10 still must freeze bounded summary/output schemas, annotations,
  optional wait and detail behavior against the real target client.

Keep fixtures, examples and corpus synchronized with the codecs. The current
15-tool corpus exceeds the catalog/Pair upfront proxy budgets; M10 must measure
actual model-visible counts and optimize without removing guards or increasing
calls. M8's freeze does not constitute MCP/Claude acceptance.

## Operation lifetime and recovery

Same key/same request resolves the retained operation; changed arguments
conflict. Preserve known-duplicate behavior at capacity. Revocation before
admission prevents effects even if validation previously succeeded. A refreshed
bearer cannot turn an old key into a new operation.

Discover/probe may wait within their existing supervised deadlines for a
bounded observation; otherwise return a receipt. Lookup waits at most ten
seconds with bounded waiter capacity. Disconnect/cancellation releases only
the waiter. An admitted effect belongs to the service even if its response
is lost; only explicit `cancel_wake` changes the permitted Wake wait path.

Never automatically resubmit after a lost response, cancellation, restart or
unknown outcome. Query the original key within retention. Changed epoch or
expired retention remains unknown; a new key requires a new explicit intent.
Retained policy failures include their operation reference even when reported
as tool failures. Untrusted labels and TV metadata remain data, not instructions.

Bound summary pages, strings and discovery candidates using shared limits;
expose explicit truncation and a usable cursor/detail path without hiding the
selected TV. Preserve complete partial-cleanup facts. Logs omit bearer and
pairing tokens, submitted host/MAC, private text and raw bodies.

## Connection action and helper contract

Implement the header helper as an early mode of the same signed executable,
reusing M8 protected bootstrap. The user-scoped HTTP entry stores a stable
per-user loopback `/mcp` URL and an absolute, safely shell-quoted installed
helper command. Verify the installed CLI's supported config writer before
choosing CLI invocation versus a narrow atomic edit.

Inspect user/local/project precedence and conflicting entries, preserve
unrelated configuration and reject concurrent clobbering. An identical Connect
is idempotent. Missing CLI, managed restrictions, malformed config and unsupported
helper support produce visible actionable failures. Failed installation leaves
a recoverable state and never reports connected. Tests use isolated config
directories; planning changes no real client configuration.

The helper authenticates the signed peer, starts an absent server or attaches
to readiness, and obtains a revocable MCP control credential under stable
client identity. It cannot Stop, Resume, replace an incompatible process or
read pairing tokens. Honor blocked credential access, sticky Stop and hostile
or occupied endpoints. Pin the configured URL; collision cannot silently move
an active Claude session. Explicit reconfiguration requires reconnection.

Helper stdout is only header JSON; errors use nonzero exit and sanitized
stderr. No bearer is stored in files, arguments, environment or logs. Validate
context against trusted installation/bootstrap state, not caller URL/name or
working directory. Avoid shell lookup of an untrusted executable. Use finite
attach/launch timeouts with no unattended credential-prompt loop. Exercise
Claude Code's one-time 401/403 refresh before admission, then resolve using
the same identity/key.

Test GUI-spawned and Claude-spawned launches for macOS Local Network
responsible-code attribution and denial/recovery. Fake TVs prove protocol
behavior but cannot prove OS consent attribution. Record signed identity,
OS/client/build versions, prompt counts and sanitized results using M6's
evidence format.

## Delivery slices and acceptance evidence

| Slice | Reviewable deliverable | Required evidence |
| --- | --- | --- |
| 1. Handoff/schema freeze | Accepted shared interfaces, catalog mappings and limits | Fixtures for every tool/recovery route and explicit predecessor blockers. |
| 2. Protocol/catalog/reads | Routing, discovery, deterministic schemas, list/inspect/lookup | Proposed `tests/mcp_protocol.rs` and `tests/mcp_catalog.rs`: framing/version/status matrix, private cache and schema/result conformance. |
| 3. Mutations/outcomes | Typed adapter and compact results | Proposed `tests/mcp_tools.rs`: all-tool fake-service parity and fake-TV one-write/packet, stale selection, expiry and partial-cleanup evidence. |
| 4. Connection/helper | Signed helper composition and opt-in GUI action | Proposed `tests/mcp_connection.rs`: isolated config preservation/conflicts/quoting, Stop, absent server, hostile peer, crash, rotation and retry. |
| 5. Installed acceptance | Signed client connection, corpus report and review | GUI closed/server absent: discover/list, read and mutation through fake TV; seven workflows in both search modes, plus signed Local Network review. |

Cross-client tests race GUI selection against MCP admission, edit Wake config
after admission, cancel/disconnect lookup while an effect runs, and rotate or
revoke credentials before admission. Exercise lost receipts, same-key changed
arguments, duplicates at capacity, cross-principal lookup, restart/expiry
unknowns and stale cached catalogs after Stop/build change. Use deterministic
fake ports/clocks and isolated subprocesses; automated tests require no TV or
local network.

Re-run the [M6 corpus proxy](milestone-06-mcp-budget.md) after schema changes.
Actual client gates remain: catalog/instruction ≤1,900 tokens, ordinary receipt
≤180, read summary ≤300, safety/detail exception ≤450, complete workflow
≤1,700 with tool search and ≤2,800 upfront. Retain the corpus call counts;
report extra slow-effect lookups, model decisions and human turns separately.
Record model/CLI versions, actual visible definitions, both result channels,
token-count method and cache conditions. A byte estimate is not actual token
evidence. Document safety/compatibility budget exceptions explicitly.

Before a PR run the [required Cargo checks](../contribution-guide.md#required-validation).
At completion add `milestone-10-review.md` mapping every overview checkbox to
commands, fixtures or signed evidence, and identify any unverified gates.
Update phase/changelog, current architecture/UX, tested connection instructions
and corpus results. M11 receives remaining upgrade/session/hardware work;
M10's own signed endpoint/helper/client acceptance cannot be deferred as Done.
