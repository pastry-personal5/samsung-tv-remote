# P2-M9: GUI API Migration

Status: Planned

## Approach

Keep Iced's window state, keyboard and pointer mapping, focus, local drafts,
and display text in `presentation/iced`. Add a typed client adapter that owns
HTTP calls, local authentication, event subscription, reconnect, and
snapshot/resync. UI messages carry API operation IDs and safe result types,
not TV sessions, Keychain objects, or Samsung frames.

Use an MVVM-inspired presentation boundary without forcing two-way binding
onto Iced: views render the view model and emit messages; `App::update` maps
those messages to local state changes or typed client calls; client responses
and events update the projection. The view model owns navigation, drafts,
dialog state, wording, and a revision-tagged projection of server facts, not
selection or connection authority. Its displayed enablement is advisory;
the server validates every command again. Keep wire DTOs and display models
distinct, and do not recreate server policy for disconnected Power Toggle,
Play/Pause, Pairing, or queue admission in the GUI.

On startup, connect to the existing per-user server or start it through the
packaged on-demand launch mechanism, then consume the event stream's initial
snapshot. A revision gap or changed server epoch triggers a new snapshot;
known operation IDs or client keys may be queried while retained. A lost
response or restarted server never causes automatic resubmission of a TV
side effect. Do not start again after an explicit Stop; keep the open GUI in a
stopped state until the user resumes. GUI close does not send Stop.

Move one vertical GUI flow at a time in isolated test builds and keep parity
tests for the visual remote, disconnected Power Toggle, Wake Steps, Pairing
confirmation, and forget. Switch the shipped GUI to the server only after
every flow has moved; never run mixed ownership against a real profile. Build
and sign the first bundle containing the GUI and the M6-selected server
process shape in this milestone. Test a synthetic GUI-created Keychain item
under that signed server before any production cutover; if legacy access
requires authorization,
present a safe migration path rather than silently re-pairing or discarding
the saved TV. The chosen signed process shape must meet the silent credential
access contract: no routine prompt and no unattended prompt/retry loop. Before
the server opens production preferences or Keychain items, ensure the previous
single-process GUI has quit. At the milestone end, remove
the temporary in-process adapter and enforce that GUI imports cannot reach
TV protocol or persistent storage adapters. P2-M11 will harden installation,
explicit stop, upgrade, and uninstall behavior.
