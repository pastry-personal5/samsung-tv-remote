# Phase 2: Local Control Server and API Clients

Status: Active

## Goal

Complete the separation of TV control from the macOS GUI. A background Control
API Server owns discovery, certificate checks, pairing, saved TVs, the one
server-wide selected TV, connections, remote commands, Wake, persistence, and
all request outcomes. The GUI is a client of that server. An MCP server exposes
the same operations through Streamable HTTP. Both surfaces use the same
application policy and cannot create a second TV-control path.

The server listens only on this Mac's loopback interface. It remains available
after the GUI closes. Remote, Wake, and connect commands name a saved TV and
the selection generation; the server rejects a target or generation that no
longer matches its selected TV. Discovery and first Pairing instead use
attempt and observation IDs. Changing selection is a separate operation
visible to every client.

See the [target architecture](target-architecture.md) for ownership, runtime,
API, security, and migration contracts. It is a proposed design; the
[current architecture](../architecture.md) continues to describe shipped code
until each migration milestone lands.
The [Control API Server internals](control-server-internals.md) specify the
planned state owner, command/effect path, operation registry, event model,
storage fences, and process boundaries.
The [server lifecycle and security research](server-lifecycle-security.md)
records the on-demand launch options, secret-access migration gate, required
macOS permissions, and unavoidable first-use consent.

## Milestones

| ID | Milestone | Deliverable | Status | Record |
| --- | --- | --- | --- | --- |
| P2-M1 | Capability catalog and proof model | Superseded proposal. | Dropped | [Archived plan](../archive/phase-2/phase-2.md) |
| P2-M2 | Sources | Superseded proposal. | Dropped | [Archived plan](../archive/phase-2/phase-2.md) |
| P2-M3 | Apps | Superseded proposal. | Dropped | [Archived plan](../archive/phase-2/phase-2.md) |
| P2-M4 | Text Input | Superseded proposal. | Dropped | [Archived plan](../archive/phase-2/phase-2.md) |
| P2-M5 | Integrated resilience and accessibility | Superseded proposal. | Dropped | [Archived plan](../archive/phase-2/phase-2.md) |
| P2-M6 | Architecture and contract | Process boundary, API and MCP contracts, baseline and migration map. | Active | [Overview](milestone-06-overview.md) · [Architecture](milestone-06-architecture.md) |
| P2-M7 | Headless control runtime | TV workflow and effects run independently of Iced. | Done | [Overview](milestone-07-overview.md) · [Architecture](milestone-07-architecture.md) |
| P2-M8 | Control HTTP server | Same-executable on-demand server, complete authenticated API/events, fresh isolated profile and early GUI probe. | Done | [Overview](milestone-08-overview.md) · [Architecture](milestone-08-architecture.md) · [Review](milestone-08-review.md) |
| P2-M9 | GUI API migration | Complete typed GUI client, outcome recovery and signed fresh-profile cutover. | In progress | [Overview](milestone-09-overview.md) · [Architecture](milestone-09-architecture.md) |
| P2-M10 | Streamable HTTP MCP | Full supported setup and control operations through `/mcp`. | Active | [Overview](milestone-10-overview.md) · [Architecture](milestone-10-architecture.md) · [Implementation](milestone-10-implementation.md) |
| P2-M11 | Integration and refactor completion | Cross-client acceptance, signed lifecycle/upgrade/removal, boundary audit, accessibility and hardware review. | Active | [Overview](milestone-11-overview.md) · [Architecture](milestone-11-architecture.md) · [Acceptance matrix](milestone-11-acceptance.md) · [Implementation](milestone-11-implementation.md) · [Review](milestone-11-review.md) |

Work in order. A milestone can start only when its required predecessor contract
is stable; deferred verification must be recorded rather than assumed passed.
The owner's 2026-11-12 instruction to complete P2-M10 authorizes implementation
against the stable M8/M9 service boundary while M9's native acceptance remains
open. This exception permits development and isolated validation; it does not
waive M9's dependent production/signing/credential gates or M10 installed-client,
model-budget and Local Network evidence. The later
[owner decision](#owner-decisions) waives only the rows it names; model
budgets are not waived.
M11 implementation uses these stable contracts while preserving every dependent M9/M10
gate. Its deterministic fixture preparation may proceed independently; signed
integrated acceptance waits for the corresponding predecessor delivery and
evidence. Implementing M11 does not accept or reassign unfinished M9/M10 work.

The old planned IDs remain Dropped rather than being reused, as required by
the [development process](../development-process.md). Each new milestone has
an overview and architecture record before implementation.

## Migration rules

- Preserve Phase 1's existing behavior and honest outcome vocabulary: admitted,
  queued, not sent, written, uncertain, and observed are different facts.
- Keep certificate pinning, TV-approved pairing, Keychain token storage,
  local-route Wake policy, one-packet Wake, bounded command admission, and
  no automatic replay after an uncertain write.
- Preserve saved-device IDs, preferences, trust records, and Keychain accounts.
  Any necessary format migration must be explicit, reversible through a
  documented backup, and tested against existing records.
- The server owns the selected TV. GUI view/navigation state stays in the GUI;
  changing selection requires the generation the client observed, and other
  clients cannot silently retarget a pending command.
- MCP can perform discovery, pairing, TV selection, configuration, control,
  and forgetting a TV. Sensitive operations must preserve the same explicit
  confirmation steps and typed validation as the GUI. An MCP tool's
  confirmation argument is not proof that a person approved it.

## Owner decisions

- **2026-11-14: evidence waiver.** A second disposable macOS account and a
  separate-host LAN fixture are unavailable. The owner waives these rows across
  M9, M10 and M11. Each is reported `waived`, never `passed`:

  | Row | Substitute evidence | Residual risk |
  | --- | --- | --- |
  | Second account / fast user switching (M11 N5) | Deterministic cross-UID peer/owner rejection; per-user runtime-directory tests | Real session switching is unobserved. |
  | Separate-host Local Network attribution, deny/revoke/restore (M9, M10, M11 N7) | Same-Mac fixture protocol evidence; observed target-TV traffic (M11 N10) | Denial/revoke recovery and server-process attribution are unproven natively. |
  | Disposable-account production helper/client smoke (M10, M11 N8) | Signed isolated harness with isolated `CLAUDE_CONFIG_DIR`; development-namespace helper tests | Production identity/Keychain path is unobserved in a fresh account. |

- **2026-11-14: removal scope.** M11 adds no prepare-to-remove feature. Removal
  is the existing Disconnect, then Stop, then moving the bundle to Trash. That
  sequence is tested and documented.
  The native check (M11 N11) runs on the owner's signed installation. It is
  hands-on, needs consent at that session and ends with Resume, Connect and
  Verify.
- **Pending: MCP budget.** M11 slice 1 measures what the installed client
  actually shows the model. The owner then decides whether to trim the catalog
  or re-baseline any gate it exceeds, and records that here. Phase 3 tool
  additions wait for this decision.

## Phase exit criteria

- The installed GUI and MCP client can each exercise every Phase 1 TV setup
  and control operation through the Control API Server. Closing the GUI does
  not stop an active server or make MCP unavailable.
- The GUI contains no direct Samsung, SSDP, Wake, Keychain, preferences, or
  application-coordinator control path. The server is the only process that
  opens TV connections and writes TV-related persistent data.
- Two clients receive consistent selected-TV state and operation outcomes;
  selection changes cannot retarget a command. Within the documented
  retention window, repeated operation keys cannot write twice. After a
  restart or an expired outcome, clients report uncertainty and do not
  automatically retry a TV side effect.
- HTTP and MCP endpoints enforce the local access policy, authenticate
  read and write operations, validate request targets, and do not leak tokens,
  private text, or TV addresses in logs.
- The MCP tool catalog and ordinary results meet the M6 model-visible token
  and workflow-call budgets, or a recorded owner re-baseline, in the installed
  Claude Code target, including
  its all-tools-upfront fallback. Savings never remove safety preconditions,
  confirmation, or honest unknown/uncertain outcomes.
- Routine GUI and configured MCP use needs no manual server command, port
  choice, or token copy. GUI close leaves the server available; an intentional
  Stop is respected. The plan does not bypass macOS privacy/Keychain prompts,
  TV Pairing approval, or a one-time choice to connect an MCP client.
- Deterministic tests cover protocol and pairing boundaries. The required
  Cargo gates pass, the signed macOS bundle runs the server and GUI, and a
  sanitized target-TV matrix records what was actually verified on hardware.

## Deferred product features

The prior Phase 2 plan for Sources, Apps, and Text Input is
[archived](../archive/phase-2/phase-2.md), not implemented. Its scope is planned
as [Phase 3](../phase-3/phase-3.md), after this server boundary is complete. No
new source, app, text-input, SmartThings, Matter, or generic TV-state claim is
part of this phase.
