# P2-M10: Streamable HTTP MCP

Status: Planned

## Approach

Place MCP transport and tool definitions under `src/server/mcp/` inside the
control server process. The adapter validates JSON-RPC, MCP transport headers,
protocol version, client credential, Origin, tool names, and typed arguments.
It translates each tool call to the same application service command or query
used by the control HTTP adapter. The server-wide selected TV and generation
remain authoritative across GUI and MCP clients.

The selected [Streamable HTTP
revision](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
must be implemented as specified for that revision. The current 2026-07-28
revision uses POST and no longer defines the earlier GET stream or
protocol-level sessions; support for an older Claude Code revision requires
implementing its separate versioned behavior. For the 2026 revision, validate
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
