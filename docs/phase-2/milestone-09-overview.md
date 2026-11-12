# P2-M9: GUI API Migration

Status: Planned

## Goal

Make the Iced GUI a complete client of the running Control API Server while
preserving the existing Power, Remote, Settings, and message behavior.

## Scope

- Add a typed local API client and move GUI startup, restore, reconnect,
  discovery, probe, Pairing, selection, Wake configuration, control, and
  forget to it.
- Project server snapshots and operation outcomes into the view model;
  resubscribe and resnapshot after a gap or server restart.
- Keep Iced views and messages for interaction, but make `App::update` a thin
  GUI coordinator over local state and a typed `ControlClient`; it must not
  own a TV control runtime or infer a server operation's terminal outcome.
- Show server unavailable, starting, and reconnecting states without claiming
  that an interrupted command failed or succeeded.
- Add the first signed bundle with the M6-selected server process shape and
  on-demand server start path so
  the real-profile cutover can be reviewed in this milestone. An intentional
  Stop is visible and is not undone by GUI reconnect.

## Completion checklist

- [ ] Every Phase 1 GUI operation reaches the server through the control API
  and preserves its present safety gates and user-facing outcome wording.
- [ ] View-model projection tests cover snapshots, receipts, terminal
  outcomes, stale selection, revision gaps, changed epoch, and unknown
  outcomes. Fake-client GUI tests verify that UI messages form requests with
  the observed target/preconditions and never send a fresh-key retry after a
  lost response.
- [ ] Selection changes from another client update the GUI and invalidate its
  old pending commands; GUI navigation remains local to the GUI.
- [ ] API outage, server restart, and resumed event stream recover without a
  client-side automatic retry or a permanent progress indicator. An unknown
  outcome is shown honestly when the server epoch changes.
- [ ] Tests use a fake API server. A signed bundle built from committed
  configuration includes the chosen signed GUI/server process packaging and
  is reviewed for first launch,
  GUI close/reopen, Pairing, Wake, legacy-item Keychain access, and Local
  Network permission. Routine token access causes no Keychain prompt. Denied,
  locked, or interaction-required access is visible, not mistaken for missing
  TV data; repeated reconnects do not create an unattended prompt loop.
  Update the contribution guide's bundle and launch
  instructions for this cutover.
- [ ] Opening the GUI needs no manual server command, port selection, or
  token copy. GUI and server credentials bootstrap through the chosen
  protected path; the current GUI launch reconnect is an observable API
  operation after attachment. Stop, crash, and disabled Local Network states
  are distinct.
- [ ] GUI code neither constructs nor imports TV or storage adapters; the
  boundary check is required before the release cutover.
- [ ] The release cutover is atomic: no shipped configuration can run a
  partially migrated GUI beside the server against the same TV profile. The
  prior GUI process is stopped before the server first opens real saved data.

## References

- [M6 acceptance and test ownership](milestone-06-acceptance.md)

- [Architecture](milestone-09-architecture.md)
- [Target architecture](target-architecture.md)
- [Lifecycle and security research](server-lifecycle-security.md)
