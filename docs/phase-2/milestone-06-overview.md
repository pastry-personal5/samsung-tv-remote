# P2-M6: Architecture and Contract

Status: Active

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
- [x] Freeze the lifecycle and local access decision with focused signed
  probes: detached launch survives GUI exit, duplicate launch/collision fails
  closed, sticky Stop requires GUI Resume, and startup has no TV effect.
  Specify Host/Origin, authenticated client identities, bootstrap, rotation,
  logout and fast-user-switch behavior. Production enforcement is M8/M9;
  cross-user and upgrade integration is M11. LaunchAgent registration is an
  optional rejected-alternative experiment, not a completion gate.
- [x] Freeze the stable per-user endpoint and Claude Code credential-helper
  contract. Record CLI version and fake-endpoint negotiation evidence, port
  collision behavior, and the active-client reconfiguration limitation.
  Production endpoint tests belong to M8 and real Claude workflows to M10.
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
- [ ] Freeze a non-destructive data/Keychain migration and rollback design.
  In an isolated account, test a synthetic old-GUI item with candidate signed
  process identities; record reads, update/delete, denied/locked access, and
  prompt counts. Choose the server process shape from evidence, or record a
  conditional choice and the exact M9 cutover blocker if the probe cannot
  represent the final identity. One explicit user-assisted migration approval
  is allowed; unattended access fails closed and routine use has zero prompts.
  Actual same-identity upgrade/migration and rollback are M9 acceptance gates.
  Development must use fake credentials or an isolated test namespace.
- [x] Crash points between Keychain, trust, and repository writes for Pairing,
  re-pairing, and forget have a durable, non-secret reconciliation design or
  an explicit fail-closed recovery procedure; an in-memory partial flag is
  not accepted as crash recovery.

Checked items mean the design is documented, not that runtime behavior
passes. M6 stays Active until focused probe results, unresolved choices and
owner acceptance are recorded. Production-server tests cannot block M6 when
that server is first implemented in later milestones. The
[acceptance and handoff record](milestone-06-acceptance.md) defines the exact
boundary, evidence format, review fixes, and downstream test owners.

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

The design is reviewable, but M6 remains Active. The focused synthetic
Keychain experiment and process-choice acceptance remain open. Fast-user
switching, Local Network privacy attribution, real signed-server Claude Code
workflows and actual token measurement are downstream implementation gates
listed in the acceptance and handoff record, not prerequisites for M6 design
completion.
