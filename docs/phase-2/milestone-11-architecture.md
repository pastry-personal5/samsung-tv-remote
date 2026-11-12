# P2-M11: Integration and Refactor Completion

Status: Planned

## Approach

P2-M9 supplies the GUI client migration and signed same-executable GUI/server
composition with a fresh server-owned profile. In this milestone, harden the
committed build and per-user launch
mechanism for installation, upgrade, explicit start and stop, crash restart,
and uninstall. A single-instance guard and authenticated readiness check
distinguish an existing server from an unrelated process occupying the
endpoint. The GUI reconnects to the server; closing its window does not
terminate the control process. Stop records a sticky paused state and closes
the listener. A separate in-app prepare-to-remove path disconnects MCP and
clears endpoint/auth state, without assuming a Trash hook or accidentally
deleting TV preferences or Keychain items. The sticky user-stopped state
prevents both GUI and Claude Code reconnect from undoing
an intentional Stop; a crash permits on-demand recovery. Neither path enables
login-time startup.

Use a per-user endpoint discovery or allocation rule that remains usable by
an explicitly configured MCP client and never attaches across macOS users.
Verify the GUI/server executable and any MCP access helper's signing identity,
Local Network permission, and fresh-service Keychain access in the actual
bundle, not only under `cargo run`. Preserve legacy records and credentials
without importing them; the owner excluded device-key migration.
Exercise denied and locked Keychain, revoked Local Network permission, and
upgrade with a stable local signing identity. The Phase 2 target is the
owner’s Mac with a local signed installation; no notarization is required.
Public distribution needs a separate future acceptance plan.

Run deterministic dual-client scenarios against a fake TV gateway. Verify
atomic subscriber attachment and ordered revisions, slow-observer resnapshot,
stale selection and changed-epoch rejection, operation lookup, finite
terminal-result retention, and recovery after server crash. In particular,
an HTTP timeout or lost MCP response is resolved by operation lookup if its
result is retained; a crash-lost or expired result is unknown. Neither client
resends the TV action automatically. Verify that Wake packet transmission,
remote socket write, and actual TV-state observation remain distinct facts.

Exercise the actual signed bundle and chosen MCP client across GUI exit and
server restart. Record only sanitized hardware evidence. Complete the module
refactor by retaining the mandatory GUI dependency boundary and verifying that
the removed in-process adapter has no replacement control path. Keep current
architecture documentation aligned with the server/client design. Preserve the
historical Phase 1 records and do not broaden Samsung capability claims.
