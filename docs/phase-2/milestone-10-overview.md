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

## Completion checklist

- [ ] `/mcp` accepts the selected revision's Streamable HTTP requests and
  rejects invalid method, headers/body metadata mismatch, unsupported
  version, Origin, authentication, or tool input with protocol-correct errors.
  There is no stdio or CLI MCP server path.
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
- [ ] Default list/discovery results are bounded summaries with a detail
  path; receipts and errors omit full snapshots and raw protocol dumps while
  retaining target, freshness, confirmation, and uncertainty facts. Operation
  lookup avoids unnecessary repeat polling where a bounded wait is supported,
  and cancelling lookup cannot cancel a TV effect.
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
  retry after header refresh.
- [ ] Protocol tests and a Claude Code smoke test cover `tools/list`, one
  read tool, and one mutating tool against a fake TV through the installed
  server and documented HTTP configuration, with the GUI closed and the
  server initially absent. The sanitized M6 workflow corpus then exercises
  tool discovery, compact results, and call counts in that actual client.

## References

- [M6 acceptance and test ownership](milestone-06-acceptance.md)

- [Architecture](milestone-10-architecture.md)
- [Target architecture](target-architecture.md)
- [Lifecycle and security research](server-lifecycle-security.md)
- [Control API Server internals](control-server-internals.md)
