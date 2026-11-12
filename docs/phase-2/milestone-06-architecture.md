# P2-M6: Architecture and Contract

Status: Active

## Credential scope update: 2026-11-12

The owner confirmed no device-key migration is needed, including synthetic
legacy migration. Earlier legacy migration/probe requirements below are
superseded by this decision. Signed fresh-credential Keychain behavior remains
M9 verification; existing items are not deleted or transferred by this scope
change. See the [current acceptance record](milestone-06-acceptance.md#owner-credential-scope-update-2026-11-12).

## Approach

Start from the current `TvControlCoordinator`, Iced `App`, dispatcher, setup
service, persistence adapters, and macOS bundle script. Record one operation
matrix: caller intent, server preconditions, long-running effects, result,
cancellation point, and what a client may claim. Include disconnected Power
Toggle's probe-then-Wake path and server-owned Play/Pause inference. Treat
pairing's observed certificate, explicit confirmation, and TV approval as
separate steps bound to one attempt.

Define one control service interface before choosing HTTP route handlers or
MCP tool handlers. Both adapters translate to typed application requests and
read the same snapshots and operation results. The service owns selected TV,
selection generation, request admission, sessions, and state revision. The
contract must make the server restart epoch visible and require it on
mutations, so a stale client cannot replay into a new server instance.

Define GUI architecture with three distinct scopes: Clean Architecture fixes
the process/module dependency direction, an MVVM-inspired view model projects
server facts plus local UI state for Iced views, and typed commands express
mutating user intents at the client/service boundary. Keep Iced `Message` and
`Task` as the GUI event/effect mechanism; they are not server commands or a
second dispatch queue. Specify explicit mappings among presentation models,
versioned HTTP DTOs, and application requests. Use the current `App` and
`ui_message` variants to identify every direct coordinator or infrastructure
dependency that must disappear from the shipped GUI in P2-M9.

Design the MCP surface as a stable semantic catalog. Compare a compact
proposal against a straightforward full-schema baseline using sanitized
scenarios: inspect selected TV, send a remote action, Wake, discover/probe/
Pair, forget, stale selection, and lost/uncertain outcome. Count the
model-visible catalog, arguments, receipts, follow-up calls, and terminal
results, not just wire bytes. Set per-category budgets from that evidence and
record how they will be measured in Claude Code with and without tool search.
Preserve explicit targets, epochs, generations, operation keys, and guarded
confirmations even when the compact version is larger than the budget.

Specify an event subscription that atomically registers the observer and
captures its initial snapshot/revision, or replays a retained cursor without
a gap. Decide slow-observer behavior and bounded retention/backpressure for
terminal results when the original single-observer acknowledgment becomes
multi-client. Mutations carry a client-scoped operation key. Document the
same-key/different-payload conflict, result expiry, changed server epoch,
and an unknown result after crash; never imply exactly-once TV execution.

Prototype detached on-demand launch in a signed bundle and document the
rejected [lifecycle options](server-lifecycle-security.md). LaunchAgent
registration is optional evidence, not a prerequisite for the chosen design. Fix a persistent per-user MCP URL and an on-demand process launch path
that survives GUI exit without login startup; test collision and fast-user
switching, not just a free port. Define sticky explicit Stop versus crash,
logout, and GUI close. Choose a credential bootstrap that does not expose
a token through an unauthenticated HTTP route, distinct authenticated GUI and
MCP client identities, plus storage, rotation, and revocation. Document
whether the MCP endpoint uses the optional OAuth authorization profile or a
restricted local credential mode, and verify Claude Code can send the required
credential and protocol revision. The current Keychain service name is shared
across preference directories, so development isolation needs a fake
credential port or a test-only namespace.

The owner selected on-demand background launch using the same signed
executable in `--server` mode on 2026-11-12. Use synthetic Keychain items
created by the old signed GUI to verify the selected identity. Measure legacy
access, upgrades, and whether noninteractive reads fail closed rather than
triggering a prompt loop. The distinct helper remains historical comparison
research, not the selected implementation. Inventory
macOS Local Network privacy, responsible code, signing stability, and sandbox
entitlements before accepting the migration design. One explicit user-assisted
migration approval is allowed; routine operation must not prompt. Distinguish
one-time OS
or TV consent from routine operation that the app can automate.

Use an explicit migration table and sequence diagrams in the target design.
Review the API contract against fake transport scenarios before moving code.
Use the [acceptance and handoff record](milestone-06-acceptance.md) to separate
M6 design/probe evidence from M7–M11 implementation tests. Production HTTP,
MCP, signed upgrade, and real-profile rollback are downstream gates.
Keep real TV addresses, certificates, tokens, and network captures out of
fixtures and review documents.
