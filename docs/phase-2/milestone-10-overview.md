# P2-M10: Streamable HTTP MCP

Status: Planned

## Goal

Expose the full supported setup and control service to an MCP client through
the background server's `/mcp` Streamable HTTP endpoint.

## Scope

- Implement the MCP transport revision and authentication behavior approved
  in P2-M6, with `tools/list` and typed tool results.
- Include tools for saved-TV/state inspection, discovery, probe, Pairing and
  re-pairing, selection, reconnect, Wake configuration, remote actions, Wake,
  operation lookup, and forgetting a saved TV.
- Use a stable, concise semantic tool catalog and bounded result shapes.
  Measure model-visible token and call costs for routine control and setup
  workflows against the P2-M6 baseline, including tool-search-off clients.
- Offer a one-time, opt-in **Connect Claude Code** action that installs a
  token-free user-scoped HTTP configuration. Its bundled signed auth helper
  supplies a client credential and starts/attaches to the server on demand;
  no manual server command or token copy is required.

## Entry and delivery gates

Planning uses M6's contract, the completed M7 runtime and
[M8's accepted isolated API/client/lifecycle handoff](milestone-08-review.md).
M9's signed production cutover remains unaccepted; M8 fake-process evidence does
not accept an installed MCP server. M10 remains Planned.

Reuse M8's frozen command/query boundary, ID/counter codecs, stable identity,
private lookup, limits and protected attach/Stop contract. Freeze the proposed
15-tool catalog's final schemas/annotations and real client budgets as listed
in the architecture; executable corpus fixtures already match M8's wire.
Installed acceptance requires M9's signed executable, fresh-profile and
credential gates. Work ahead of a required predecessor needs a recorded
phase-plan exception; planning does not grant one.

Target only `2026-07-28` initially, as selected by M6. Recheck the installed
Claude Code version against the real endpoint. The helper reuses the chosen
same-executable signed bundle and M8 bootstrap; MCP cannot Resume or replace
a server. No client configuration changes are part of planning.

## Delivery order

1. Freeze predecessor contracts, the protocol status/metadata matrix, shared
   pool allocation and the proposed 15-tool schemas/service mappings. Verify
   the isolated target CLI/runtime and header-helper compatibility early.
2. Implement authenticated revision discovery, JSON-RPC framing, deterministic
   catalog/schema and read-only tools against fake ports.
3. Add typed mutations and compact outcomes; prove confirmation, deduplication,
   cancellation and cross-client selection behavior.
4. Implement the signed header-helper mode and opt-in GUI connection action,
   with isolated configuration tests before installed-client review.
5. Run signed Claude Code smoke/workflow tests, measure actual context/calls
   with tool search on/off, then write a completion review and M11 handoff.

The architecture assigns boundaries and evidence to these slices. No new
source/app/text-input features, generic executor, remote hosting, OAuth
service, login registration or Claude Desktop integration is included.

## Completion checklist

- [ ] `/mcp` accepts the selected revision's Streamable HTTP requests and
  rejects invalid method, headers/body metadata mismatch, unsupported
  version, Origin, authentication, or tool input with protocol-correct errors.
  Required per-request metadata, result discriminators and direct first-call
  operation without discovery are covered. There is no stdio MCP server path.
- [ ] HTTP and MCP share the existing connection/request/subscriber limits.
  POST reads use reserved query capacity; bounded lookup waits leave immediate
  recovery capacity available. Mixed-client saturation and disconnect tests
  prove reservations are released and no mutation bypasses admission.
- [ ] Every supported TV operation reaches the same application service used
  by `/v1`; no tool opens a TV socket or bypasses selection, Pairing, or the
  bounded dispatcher.
- [ ] Pairing requires a current observed certificate and explicit matching
  confirmation; forgetting requires explicit target confirmation. A second
  tool call does not itself prove human approval. No token or real local
  address appears in tool schema text, logs, or examples.
- [ ] Tool receipts and operation lookup preserve `not_sent`, `written`, and
  `uncertain` remote outcomes and distinct Wake progress. Mutating tools
  require server epoch and operation key; lost responses and client
  cancellation do not trigger an automatic second command.
- [ ] One typed remote-action tool covers the supported semantic actions;
  there is no per-button tool proliferation or generic execute/batch escape
  hatch. Tool definitions are concise, searchable, deterministic, and
  independent of current TV connection/selection state. Read-only and
  destructive annotations are accurate but never used as authorization.
  Mutations explicitly disable read-only/idempotent hints; installed-client
  disconnect tests prove annotations do not induce mutation replay.
- [ ] Default list/discovery results are bounded summaries with a detail
  path; receipts and errors omit full snapshots and raw protocol dumps while
  retaining target, freshness, confirmation, and uncertainty facts. Operation
  lookup avoids unnecessary repeat polling where a bounded wait is supported,
  and cancelling lookup cannot cancel a TV effect.
  Saved-target and operation-result detail/pages are reachable through the
  frozen catalog, preserving complete guards, recovery markers and expiry.
- [ ] The negotiated MCP revision's cache and structured-result rules are
  tested. Compare terse text with structured-plus-compatible-text results in
  the actual client; do not assume structured output or HTTP compression
  saves model tokens. Report catalog, per-call, and end-to-end workflow token
  estimates and call counts against the M6 budgets, with tool search on and
  off. Cached tool discovery cannot bypass first-use startup, sticky Stop,
  authentication, or changed-epoch rejection. Any budget exception has a
  safety or compatibility reason.
- [ ] The user-scoped Claude Code HTTP entry has a stable per-user URL and
  dynamic header helper, not a literal token or a project `.mcp.json` edit.
  Existing unrelated client configuration is preserved; conflicting local or
  project entries are detected, not silently overwritten. The helper respects
  explicit Stop, handles crash recovery and credential rotation, and emits
  no pairing token. GUI-spawned and Claude-spawned server launches are checked
  for the correct macOS Local Network responsible code. Authorization
  failures are rejected before effects, including Claude Code's one-time
  retry after header refresh. Concurrent helpers reuse the current credential
  and MCP identity. Multiple Claude sessions share that identity's quotas and
  private lookup scope; conversation isolation is not claimed.
- [ ] Connect reports configuration saved and endpoint verified separately.
  Disconnect removes only the unchanged app entry and revokes MCP access;
  restart/recheck/helper execution cannot silently restore it. Partial
  installation/removal, writer conflicts and app relocation have tested repair
  paths. GUI control stays usable.
- [ ] Protocol tests and a Claude Code smoke test cover `tools/list`, one
  read tool, and one mutating tool against a fake TV through the installed
  server and documented HTTP configuration, with the GUI closed and the
  server initially absent. All nine current M6 corpus scenarios run in both
  tool-search modes, with additional all-tool/re-pair/cleanup parity tests.
  Evidence names the actual signed executable/profile/composition; fake
  development mode cannot satisfy production-helper or OS permission gates.

## Plan review

The [2026-11-12 plan review](milestone-10-plan-review.md) records corrected
contracts and implementation gates. This is documentation review only; it
does not accept protocol, signed-client or token-budget evidence.

## References

- [M6 acceptance and test ownership](milestone-06-acceptance.md)
- [M6 token and workflow budgets](milestone-06-mcp-budget.md)
- [Sanitized workflow corpus](mcp-corpus.json)
- [Architecture](milestone-10-architecture.md)
- [Target architecture](target-architecture.md)
- [Lifecycle and security research](server-lifecycle-security.md)
- [Control API Server internals](control-server-internals.md)
