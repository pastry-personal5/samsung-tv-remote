# P2-M7: Headless Control Runtime

Status: Planned

## Approach

Extract the workflow in reviewable slices, keeping the existing application
ports and Samsung/macOS adapters. The core runtime accepts typed intents,
starts bounded asynchronous effects, and feeds completions back through one
state owner. Iced temporarily calls this runtime in process; that adapter is
removed after P2-M9.

Give each mutating use case a typed application command and handler, with
explicit target and freshness preconditions where applicable. Keep snapshot,
operation lookup, and subscription as queries rather than pretending they are
TV actions. This is command handling inside the service, not a generic command
bus, event-sourced write model, or per-button GUI command object. The same
handlers must later serve HTTP and MCP adapters.

Replace public mutable `TvControlCoordinator` fields with focused methods and
immutable snapshot types. Keep Samsung frames and sockets in infrastructure,
and Keychain and preference access behind application ports. Remote dispatch
remains the sole queue; moving effect scheduling must not add a second queue
or bypass its admission and terminal-result rules.

The current journal is acknowledged by one GUI observer. Introduce server
operation IDs, bounded outcome retention, and revisioned snapshots/events so
one client cannot erase another client's result. Register a subscriber and
capture its snapshot/revision in one state-owner operation, then send only
later revisions. A bounded stream may drop a slow observer only with an
explicit resnapshot path; it may never skip an event unnoticed.

Scope operation keys to client identity and server epoch. Reject a repeated
key carrying a changed payload; within the documented retention window,
return the original receipt or outcome for a repeated key. After restart,
reject stale-epoch mutations, report unresolved writes as unknown, and
prohibit automatic replay. Keep Wake progress and remote-write outcomes as
different types. Characterization tests use fake TV sessions, Wake transport,
storage, and pairing.
