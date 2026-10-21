# P2-M8: Control HTTP Server

Status: Planned

## Approach

Add a server composition root under `src/bin/` and HTTP adapters under
`src/server/control_http/`. The server constructs the application runtime
with the existing Samsung, SSDP, UDP Wake, preferences, trust, and Keychain
ports. HTTP handlers validate and translate requests; they cannot change
selection, dispatch, or Pairing state except through typed service methods.
Set or document Cargo's GUI default-run behavior when the second executable
is added, and give explicit development commands for both binaries.

Expose versioned JSON resources and a separate server-sent event response.
Every mutation has a client operation key and explicit target/precondition.
Return an admission receipt promptly for long-running work, then make its
terminal result queryable by operation ID or client key. Register an event
subscriber and capture its initial snapshot/revision atomically; send only
later revisions. On a slow consumer or broken stream, force a new snapshot
instead of silently dropping events. Retention is bounded independently of
any one client's acknowledgment. Map typed validation, conflict, busy, and
unknown-outcome errors to versioned HTTP status and JSON bodies.

Bind to loopback and check Host, Origin, and a local credential on every
protected route. Authentication bootstrap must not expose a credential from
an unauthenticated localhost endpoint. Use typed, sanitized errors and fixed
request/body limits. Implement the per-user endpoint manifest, single-instance
guard, authenticated readiness, and sticky user-stopped state defined by the
[lifecycle research](server-lifecycle-security.md). A later GUI milestone
starts this same executable when the user opens the GUI. No login-time server
process or registration is part of the baseline.

Until P2-M9 completes, the existing GUI still owns an in-process runtime.
Run the new server against fake TV and credential ports plus isolated
non-secret storage; the current Keychain service name is global to the user
and is not isolated by a different preferences directory. Do not package or
point both processes at the same real saved-TV data. The first real-profile
server launch belongs to the atomic GUI cutover in P2-M9.
