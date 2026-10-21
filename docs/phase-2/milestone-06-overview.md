# P2-M6: Architecture and Contract

Status: In progress

## Goal

Turn the server-first product decision into a reviewable process design and a
versioned control contract before moving code. Capture Phase 1 behavior so
the refactor has explicit parity checks.

## Scope

- Document process ownership, launch and shutdown, the local access boundary,
  a stable per-user endpoint, user-stopped and crash states, GUI and MCP
  credential setup, and how one
  server-wide selected TV behaves when two clients act concurrently.
- Specify requests, snapshots, event revisions, operation IDs, terminal
  outcomes, error codes, cancellation, and recovery after an HTTP timeout or
  server restart. Include discovery, certificate observation, Pairing,
  reconnect, disconnected Power Toggle, inferred Play/Pause, Wake, remote
  actions, configuration, and forget.
- Choose and record supported MCP protocol revisions and a real target client.
  Streamable HTTP is required; a CLI or stdio MCP server is outside scope.
- Design a compact MCP catalog that still covers every supported operation.
  Record a sanitized workflow corpus, a token-counting method, and budgets
  for tool definitions, default results, and complete workflows before M10.
- Inventory current coordinator and Iced responsibilities, persistent file
  versions and Keychain identifiers, bundle scripts, and Phase 1 gaps.
- Define the GUI presentation contract: distinguish Iced messages, local
  view-model state, typed client requests, server commands/queries, and
  receipts/events. Record which current `App` and `Message` variants move to
  the server and which remain local to the GUI.
- Review the server-internal design's single state owner, bounded effect
  workers, durable-write fences, operation registry, event fan-out, and
  shutdown ordering before implementation.

## Completion checklist

- [x] The target architecture and API examples cover every existing TV
  operation. Remote writes, Wake packet transmission, and observed remote
  readiness have distinct outcome types.
- [x] A concurrent-client sequence shows selection change, stale generation,
  and one in-flight uncertain write without automatic replay. Selection
  itself has an expected-generation precondition.
- [x] The contract specifies server epoch, client-scoped operation keys,
  same-key/different-payload rejection, bounded retention and expiry, and
  the unknown outcome after a crash. It makes no exactly-once claim after
  the retention window.
- [x] The event subscription delivers an atomic initial snapshot plus later
  revisions, or proves retained replay from a snapshot cursor; a slow client
  has an explicit resynchronization path.
- [ ] Threat and failure cases include loopback exposure, Origin validation,
  distinct authenticated GUI/MCP client identities, credential bootstrap and
  rotation, fast user switching, duplicate process startup, crashes, and
  malformed requests. Compare detached on-demand launch with an app-bundled
  LaunchAgent in a signed prototype; decide how the server survives GUI exit,
  respects explicit Stop, and remains absent at login. The chosen MCP
  revision, authorization mode, and Claude Code test target are recorded
  without claiming untested compatibility. Record the minimum supported
  macOS version before relying on a ServiceManagement or security API.
- [ ] The endpoint decision covers a persistent per-user MCP URL, port
  collision, unrelated listeners, fast user switching, and the limitation of
  reconfiguring an already-running HTTP MCP client.
- [x] The contract separates server process startup from TV network effects.
  MCP-only connection does not discover, pair, connect, or Wake a TV; the
  current GUI's launch reconnect is modeled as its own operation after attach.
- [x] The proposed MCP catalog uses semantic tools rather than one tool per
  remote key or an opaque execute/batch tool. Sample short success, stale,
  uncertain, Pairing, and forget results retain target and confirmation
  context. Budgets include tool-search-on and all-tools-upfront cases, model
  round trips, and the cost of structured-plus-text compatibility output.
- [x] The migration map identifies which current module owns each behavior,
  its target owner, and an acceptance test or manual check.
- [x] The migration map includes the GUI state-ownership table and one complete
  user-action sequence from view event through control command to revised
  view-model projection. It identifies the source of target ID, epoch,
  generation, operation key, and unknown-outcome wording without introducing
  a second queue or automatic retry.
- [x] The server-internal design fixes ownership and linearization for command
  admission, deduplication, durable selection, late effect completions, and
  atomic subscription. Remaining numeric limits and platform choices are
  explicitly assigned to a milestone rather than assumed implemented.
- [ ] Existing saved data and Keychain items have a non-destructive migration
  and rollback plan. A synthetic old-GUI Keychain item is tested with the new
  signed helper and same-signed-executable server mode; prompt, denial, lock,
  and upgrade behavior are recorded. The normal GUI/MCP launch and token-use
  path has zero routine prompts; an interaction-required result blocks without
  an unattended prompt or reconnect retry loop. No token-store downgrade is
  accepted to meet that target.
  The development profile cannot access production Keychain items. Required
  Local Network, signing, and any sandbox entitlements are inventoried. No
  production implementation or client compatibility claim is inferred from
  a prototype.
- [x] Crash points between Keychain, trust, and repository writes for Pairing,
  re-pairing, and forget have a durable, non-secret reconciliation design or
  an explicit fail-closed recovery procedure; an in-memory partial flag is
  not accepted as crash recovery.

Checked design items mean the proposed contract is documented and reviewable;
they do not mean the server implements it or its future runtime tests pass.
The three unchecked platform and migration gates still block M6 completion.

## References

- [Architecture](milestone-06-architecture.md)
- [Target architecture](target-architecture.md)
- [Control API Server internals](control-server-internals.md)
- [Lifecycle and security research](server-lifecycle-security.md)
- [Draft control/API/MCP contract](milestone-06-contract.md)
- [API operation examples](milestone-06-api-examples.md)
- [Phase 1 baseline and migration map](milestone-06-baseline.md)
- [MCP corpus and measured proxy budget](milestone-06-mcp-budget.md)
- [Signed platform evidence and remaining gates](milestone-06-platform-evidence.md)
- [Human isolated-account test runbook](milestone-06-human-runbook.md)
- [Requirement-by-requirement review](milestone-06-review.md)

The documents above make the design reviewable, but the checklist is not
complete. In particular, the old signed GUI Keychain experiment, fast-user
switching and Local Network privacy tests, and signed-server Claude Code
interop and model-visible token measurement have not passed. The contracts
are candidates until those tests and fake-port sequence/crash tests validate
them.
