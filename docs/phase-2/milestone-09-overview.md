# P2-M9: GUI API Migration

Status: Planned

## Goal

Make the Iced GUI a complete client of the running Control API Server while
preserving Phase 1's Power, Remote, Settings, shortcuts and honest outcome
wording. Ship the first locally signed same-executable GUI/`--server` bundle
with on-demand startup and a fresh server-owned profile.

## Entry and owner decisions

M8's [completion review](milestone-08-review.md) provides the accepted isolated
HTTP/SSE, wire/access/lifecycle and reusable native-client handoff, including
process and early-GUI-probe evidence. M9 extends that boundary to full shipping
GUI parity; M8 does not establish production signed identity or Keychain access.
M6's signed peer/credential gates must have evidence before production cutover;
M9 supplies that evidence rather than assuming fake tests prove it.

The owner decisions recorded on 2026-11-12 apply:

- Use the same signed executable, dispatching `--server` before Iced. Start
  on demand, survive GUI exit, never register login launch, and honor sticky
  Stop until explicit GUI Resume.
- Start with an empty server profile in distinct data and credential
  namespaces. Import no legacy devices, selection, trust, recovery markers or
  credentials. Existing legacy data stays untouched; no device-key migration
  or synthetic migration experiment is required.
- Reuse M8's automatic background attachment policy: immediate first attempt,
  1-second exponential delays capped at a repeated one-hour interval, reset
  after 30 healthy seconds, one attempt/timer and no mutation replay.
- Require explicit restart for an authenticated incompatible server build.
  Opening the GUI cannot replace it or undo Stop.

## Scope

- Extend M8's typed local client to every GUI workflow, operation lookup and
  recovery. Move Iced off `InProcess`, `ControlService` and `AppServices`.
- Separate wire DTOs, presentation projection, pending request tracking and
  local window/draft state; keep `App::update` a thin coordinator.
- Preserve setup confirmations, target/generation/configuration preconditions,
  keyboard/pointer parity, message history and Wake progress/cancellation.
- Add visible server lifecycle and credential states, Resume, explicit restart
  and intentional Stop. GUI close only detaches.
- Compose production adapters exclusively in server mode; complete signed
  bootstrap, fresh Keychain lifecycle and Local Network verification.
- Cut over the shipping executable atomically, remove the temporary adapter,
  enforce the GUI dependency boundary and update tested bundle instructions.

No Sources, Apps, Text Input or new TV-state claim is added. MCP belongs to
M10; full session/upgrade/uninstall integration and accessibility/hardware
matrix hardening remain M11. M9 still requires its own signed first-use,
credential update/restart and GUI parity evidence.

## Completion checklist

- [ ] M8 handoff is accepted with concrete wire/client/lifecycle references;
  M6 signed gates have evidence or an explicit remaining release blocker.
- [ ] Every Phase 1 GUI operation uses the API, including initial explicit
  TV Connect after restore, discovery/manual probe, Pairing/re-pair, selection,
  Wake configuration, remote actions, Wake/cancel, Forget and setup recovery.
  Server attachment alone never claims TV readiness.
- [ ] Projection and fake-client intent tests cover snapshot/event ordering,
  receipts racing events, stale selection/configuration, revision gaps,
  changed epochs, private retained lookup, unknown outcomes and late responses.
  No lost response generates a fresh-key retry or false not-admitted result.
- [ ] External selection changes invalidate stale GUI intents and setup
  confirmations without retargeting them; navigation stays local. Dirty Wake
  drafts cannot save against a different target or unseen configuration.
- [ ] Outage/restart/resubscribe restores an atomic snapshot without replaying
  mutations or leaving permanent progress. Backoff fake-clock tests cover the
  repeated cap, healthy reset, sleep, exit, single attempt/timer, rotation and
  blocked states. Status and next retry remain visible while the GUI responds.
- [ ] Stop, starting, reconnecting, incompatible build, blocked credentials,
  unsafe endpoint and disabled Local Network states are distinct. Explicit
  Resume/restart use M8's management path; closing all GUI windows sends no Stop
  or runtime drain. Opening needs no manual command, port choice or token copy.
- [ ] Signed bootstrap authenticates the selected peer code identity before
  disclosing credentials; fake/wrong peers and replay fail. Same-executable
  identity limits are documented. No bearer is placed in files, logs, command
  arguments or environment.
- [ ] A signed bundle built from committed configuration passes empty first
  launch with populated legacy fixtures, first Pairing, new-profile restore,
  GUI close/reopen, Wake and Local Network review. Fresh TV/API credentials
  pass save/read/update/delete, server restart and signed rebuild/update.
  Routine access produces zero Keychain prompts; denied, locked and
  interaction-required states stay visible without unattended prompt loops.
- [ ] Every migrated flow has deterministic API/fake-client evidence, and
  two-client/subprocess tests prove cross-process selection and outcome
  recovery. Native GUI review records screenshots and sanitized results;
  automated tests require no TV or local network.
- [ ] The shipping GUI imports no runtime, coordinator, TV or persistence
  adapters; `in_process` is removed and a required boundary check covers GUI
  modules and composition. Production adapters are constructed only in server
  mode. Development fakes cannot open the production namespace.
- [ ] The previous single-process GUI is stopped before the new server first
  opens its production namespace. One release switches all flows together;
  there is no shipped mixed-mode fallback. Failed gates block cutover.
- [ ] Required Cargo checks pass. Update current architecture/UX and tested
  contribution-guide launch commands at cutover; write an M9 completion review
  mapping every item to evidence and recording M11 handoff and limitations.

## Delivery order

Follow the [architecture sequence](milestone-09-architecture.md#implementation-sequence):
freeze the M8 handoff; build projection/request tracking; migrate control and
setup flows in isolated builds; finish lifecycle UI; verify signed composition;
then cut over and review. M9 stays Planned until implementation starts; this
record claims no Cargo, signed-platform or TV result.

## References

- [Architecture](milestone-09-architecture.md)
- [M8 handoff](milestone-08-architecture.md)
- [M6 acceptance and test ownership](milestone-06-acceptance.md)
- [Target architecture](target-architecture.md)
- [Lifecycle and security research](server-lifecycle-security.md)
