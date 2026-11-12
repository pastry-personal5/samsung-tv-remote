# P2-M10 plan review — 2026-11-12

Status: Planning reviewed; implementation and installed acceptance pending.

Reviewed the M10 overview/architecture against M8's actual HTTP adapter,
service queries, wire decoders and executable documentation fixtures, M6's
corpus budgets, and M9's implemented but unaccepted signed production cutover.
Re-reviewed the transport and client contracts against primary documentation
and the current service/access/bootstrap code. No predecessor exception is
granted. This pass changes only M10 planning and the phase decision log.

## Findings and corrections

| Priority | Finding | Plan correction |
| --- | --- | --- |
| High | “Reuse infrastructure” did not specify how `/mcp` enters the existing concrete `HttpAdapter`; separate pools could silently double capacity. | Require shared ingress/access/reservations and `/v1`/SSE regressions before mounting the route. |
| High | Bounded lookup wait sounded available in M8, whose façade only exposes immediate queries. | Assign subscription/recheck waiting above the service, with private lookup, monotonic deadlines, finite reservations and immediate recovery capacity. |
| High | Lost receipt recovery and an unknown lookup could be mistaken for permission to repeat an effect. | Require lookup by original key, preserve receipt recovery fields, and explicitly distinguish missing/expired from non-admission. |
| High | Helper launch had no deadline tied to the client's execution limit. | Require a total deadline below the documented ten-second limit, concurrent-start convergence and termination/late-start tests. |
| Medium | Delivery order still named 13 tools despite the reconciled 15-tool corpus. | Align overview with the catalog, retaining separate recovery tools and existing budget overruns. |
| Medium | Discovery/probe waiting was not reconciled with short receipts or slow-path call budgets. | Default to receipts, freeze a bounded observation fast path and measure slow lookup calls separately. |
| Medium | Configuration installation could be mistaken for a working connection. | Separate saved/verified states; put actual CLI/runtime revision/helper compatibility in the first freeze slice. |
| Medium | Pagination, installation repair and opt-in disconnect lacked concrete guards. | Require scoped stale-detecting cursors, conflict-safe entry removal/credential revocation and relocation/repair tests. |
| High | The protocol plan omitted required per-request body metadata and the result discriminator; a legacy-shaped result could complete an RPC incorrectly or fail modern-client decoding. | Require version/capabilities on every request, `resultType: "complete"`, direct first-call tests and a normative status/error seed matrix. Keep RPC completion separate from TV-effect completion. |
| High | M8 assigns query capacity using HTTP GET, while MCP queries also use POST. Reusing it unchanged would strand recovery behind mutations. | Classify validated tool semantics, preserve existing aggregate/per-identity pools and reserve at most four global/two per-identity query slots for waits. Cover mixed-route saturation and stalled flushes. |
| High | Claude Code uses read-only/idempotent annotations to decide whether an interrupted tool call can be replayed. Retention-bounded deduplication alone cannot justify those hints on mutations. | Explicitly disable both hints on all 12 mutations and verify disconnect behavior separately from the documented authentication refresh. |
| High | M8 can revoke a live lease but provides no public persistent Disconnect/regrant workflow. Treating config removal as revocation would leave usable credentials. | Make the GUI-management/restart-persistence contract an explicit implementation deliverable; distinguish config removal from revocation and require explicit Connect to restore access. |
| Medium | A generic JSON map could discard duplicate members before the shared strict decoder sees them. | Preserve duplicate rejection through envelope/argument parsing and extract typed validation without a second policy path. |
| Medium | “Private” could imply isolation between Claude conversations, although bootstrap provisions one MCP role identity. | Document shared MCP quotas/lookup scope, reuse current credentials on helper reconnect and test simultaneous sessions. |
| Medium | A bounded summary promised a detail path without assigning it to any of the 15 tools. | Assign saved-target detail to `inspect_tv` and paginated discovery/probe/cleanup facts to `get_operation`, without rerunning effects or extending handle expiry. |
| Medium | Delivery required seven workflows, but the reconciled corpus contains nine and still does not cover every tool/recovery variant. | Require all nine scenarios in both search modes plus all-15-tool parity and explicit recovery/cancellation cases. Identify signed versus fake ports in evidence. |

## Follow-up review: implementation failure paths

A further review of `control_http/transport.rs`, `access/mod.rs`,
`bootstrap/mod.rs` and `application/control_service.rs` found these remaining
planning gaps. The overview and architecture now include the corrections.

| Priority | Finding | Plan correction |
| --- | --- | --- |
| High | A parsing reservation was required without specifying its transition into semantic pools; retaining ordinary capacity while waiting for classification could starve POST recovery reads or deadlock permit transfers. | Require one aggregate accounting model, bounded classification capacity, atomic release/transfer and mutation-saturation/slow-body evidence before route integration. Numeric allocation remains a slice-1 decision. |
| High | Persistent revocation was assigned but its crash ordering and existing-profile initialization were unspecified. Bootstrap currently enrolls loaded role credentials; refresh rotates them. | Define durable enrollment independently of Keychain presence. Disable before config removal, fence concurrent grants, honor disabled state at every startup/refresh path and inject crashes at each transition. |
| High | Explicit reconnect could reuse a revoked bearer, restoring access to an old client; a delayed helper could race a new enrollment. | Regrant with a fresh bearer under the same principal, preserve retained operation ownership/deduplication, and fence old helpers by enrollment generation. |
| Medium | Disconnect could imply cancellation of admitted work or affect only one conversation. | State that every MCP session loses new access while already admitted effects continue; recover results after explicit re-enrollment within normal retention. |
| Medium | Server-side cache invalidation sounded sufficient to remove stale installed definitions. | Partition catalog caching by build/authorization, validate every invocation independently and prohibit HTTP intermediary caching of authenticated exchanges. |
| Medium | Lookup waiting did not specify event-channel closure or validation before subscription allocation. | Validate selectors first; perform final private lookup on closure and bound resubscription by the original deadline/capacity. Add backlog/owner-shutdown cases. |
| Medium | Endpoint verification could be presented as proof that Claude loaded a new config entry. | Separate configuration saved, authenticated endpoint probe and actual client attachment; saved-but-unverified is recoverable without Resume or reinstall. |

These are implementation requirements, not evidence of implemented enrollment,
permit transfer or installed client behavior. The selected protocol revision,
15-tool scope and predecessor gates remain unchanged.

## Remaining decisions before implementation

Slice 1 must record exact protocol status/error fixtures and validation order,
final input/output schemas and annotations, concrete page/string/metadata
limits and cache TTLs, pool/waiter accounting, discovery fast-path timing and
the target CLI/runtime version. The architecture now supplies the transport
error seed matrix, existing request pool sizes and waiter ceiling. The
per-tool freeze record must name exact service mappings and test ownership.
These are deliverables, not claims that M8 already supports them. The chosen
revision remains `2026-07-28`; any compatibility fallback needs a separate
explicit decision and tests. No actual Claude configuration is changed here.

The proxy catalog and Pair upfront overruns remain open. Real model-visible
measurement must include text/structured duplication, cached and uncached
startup, both tool-search modes and slow effects. Do not relax confirmation,
identity, uncertainty or target preconditions to meet a token budget.

## External contract verification

Primary documentation checked on 2026-11-12:

- [MCP Streamable HTTP revision](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
  supports the plan's POST-only, session-free transport choice. Freeze the
  complete normative error/header table during implementation.
- [Claude Code dynamic headers](https://code.claude.com/docs/en/mcp#use-dynamic-headers-for-custom-authentication)
  documents shell execution, JSON header output, the ten-second helper limit
  and one refresh/retry after a tool-call authentication rejection. This is
  documentation evidence, not an installed-client test.
- [MCP base protocol](https://modelcontextprotocol.io/specification/2026-07-28/basic/index)
  supplies required request metadata and result framing; the plan now makes
  these explicit and separates RPC completion from an operation's outcome.
- [Claude Code interrupted-call annotations](https://code.claude.com/docs/en/mcp#mark-tools-that-are-safe-to-call-twice)
  explains why the plan must test client replay behavior, rather than treating
  annotations solely as UI descriptions.

## Earlier review validation

The earlier review recorded the following delegated validation. These figures
are historical; follow-up validation is recorded separately below:

- `cargo test --test control_documentation` — all three tests passed.
- `python3 scripts/measure_mcp_corpus.py --wire-fixtures` — valid fixtures.
- `python3 scripts/measure_mcp_corpus.py` — all nine scenarios; 15 tools,
  catalog/instruction 2,136 proxy units and Pair upfront 3,036. The existing
  1,900/2,800 overruns remain open; these are not actual model-token counts.
- `git diff --check` — clean; direct Python whitespace checks also covered
  the untracked review file. Local Markdown targets and anchors in all four
  reviewed documents resolved.

Full Cargo build/lint/test gates were not rerun for this documentation-only
task. They remain required before a PR. Concurrent M9 source edits were not
part of this review or its validation judgment.

No MCP adapter, real configuration write, signed bundle, TV workflow or actual
model-token result is claimed. M10 remains Planned. At implementation completion,
write the separate `milestone-10-review.md` required by the architecture.

## Follow-up validation

Delegated documentation validation passed:

- `cargo test --test control_documentation` — three tests passed.
- `python3 scripts/measure_mcp_corpus.py --wire-fixtures` — valid fixtures.
- `python3 scripts/measure_mcp_corpus.py` — nine scenarios and 15 tools;
  documented proxy overruns remain open.
- `git diff --check` — clean.
- Local Markdown link/anchor check — four reviewed files, no broken targets.

No source, corpus schema, client configuration or signed artifact was changed
by this review. Concurrent Makefile edits are outside its scope. Full Cargo
checks were not run and remain required before a PR; this review does not
accept M10 delivery.
