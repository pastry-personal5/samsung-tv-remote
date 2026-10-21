# Phase 2 server lifecycle and local security research

Status: Proposed design input for P2-M6; platform behavior requires signed-bundle
tests before a shipping claim.

## Product rule

No routine terminal command, manual port selection, token copy, or server-start
button should be needed. Opening the GUI starts or attaches to the per-user
server. Closing the GUI only detaches its client; the server remains available
to MCP. There is no login-time server process. An intentional **Stop server**
action is different from closing the GUI: it records a user-stopped state that
GUI reconnect and an MCP auth helper must not undo until the user explicitly
resumes. A crash is not an intentional stop; the next explicit GUI opening or
configured MCP connection may recover it. Logout ends the user session; the
server does not relaunch at the next login until a client is used.

Starting the HTTP server restores saved state but does not itself discover,
pair, Wake, or send a remote action. In particular, an MCP connection used
only to list tools must not contact or wake the TV. The current GUI requests
a secure reconnect to its saved selection on launch; P2-M9 may preserve that
behavior as a distinct GUI request after authenticated attachment, with its
own operation ID and observable outcome. No uncertain control action is
replayed as part of reconnect.

MCP-on-connect recovery counts as explicit use of a configured client, not
login startup. It is never a response to a TV command timeout: operation
lookup and unknown outcomes still prevent automatic replay of TV side effects.

## Launch mechanisms considered

| Mechanism | Advantage | Cost and disposition |
| --- | --- | --- |
| GUI or bundled auth helper starts a detached, signed per-user server on demand | No login registration or extra routine approval; matches explicit launch and persistence after GUI exit. | No OS supervisor; prove survival, crash recovery, logout behavior, and upgrades in signed-bundle tests. Preferred baseline if these tests pass. |
| App-bundled `SMAppService` LaunchAgent with on-demand configuration | OS-managed registration and launch. | Registration is subject to approval, has a `requiresApproval` state, and bootstraps at later logins. Use only if the baseline cannot meet reliability requirements and after revisiting the no-login-start requirement. |
| Login item, always-on agent, or root daemon | Can provide availability before any client opens. | Starts at login or uses broader authority than required. Not Phase 2's default. |

Apple says a registered LaunchAgent is bootstrapped on later logins and may
begin running on registration, and the user can deny or later revoke it:
[SMAppService registration](https://developer.apple.com/documentation/servicemanagement/smappservice/register%28%29),
[service status](https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.enum).
Do not describe `SMAppService` registration as a guaranteed silent, no-login
alternative. The detached-process baseline is a design inference, not an
Apple guarantee, so P2-M6 must prototype it in the signed bundle.
The launcher must spawn as the current user with a controlled environment,
closed inherited GUI descriptors, and no credential in arguments or standard
output. Its process lifecycle must be tested with the GUI fully exited, not
just its window hidden.

## Endpoint, attachment, and stop semantics

The server chooses a persistent per-user loopback port and stores its endpoint
in a non-secret, owner-only manifest. A stable port is needed for a configured
HTTP MCP URL; a transient port cannot be repaired by an auth-header helper
alone. First allocation probes conflicts. A later bind collision must fail
closed, not attach to the occupant or silently switch the MCP URL. The GUI
may offer an app-managed endpoint reconfiguration for a future client session;
M6 must test the active-session limitation. Random high-port allocation makes
collisions unlikely but cannot guarantee the same TCP port to simultaneous
macOS users. Fast user switching must be tested explicitly.

Use an owner-only runtime directory, atomic manifest replacement, a
single-instance lock, and a challenge authenticated with a per-user secret
before treating a listener as this app's server. A PID file or a successful
TCP connection alone is not identity proof; do not kill an unrelated process
on port collision. Readiness also reports API/build compatibility; an
incompatible old server drains and releases its profile before a replacement
starts. Authenticate `/v1`, `/mcp`, readiness, and event streams.
Never place credentials in endpoint files, URLs, command arguments, process
environment, logs, or crash diagnostics. Check loopback binding, Host and
Origin, and reject browser-origin cross-site requests.

| Event | Required behavior |
| --- | --- |
| GUI opens; server absent | Start once, wait for authenticated readiness, then subscribe. Concurrent GUI launches attach to the winner. |
| GUI closes | Drop only its HTTP/event connections; leave the server running. |
| Configured MCP client connects; server absent | The bundled auth helper starts or attaches before emitting a header; it respects user-stopped state. No stdio MCP transport is introduced. |
| Server crashes | Existing clients show disconnected/unknown outcomes; the next explicit client connection may start a new epoch. No TV action is replayed. |
| User selects Stop server | Stop admission, finish or mark in-flight work uncertain, close listeners, record user-stopped state; clients show stopped. Reconnect must not restart it. |
| User resumes | Clear user-stopped state through an explicit GUI action and start/attach. |
| User logs out or app upgrades | Shut down or replace the old signed server cleanly; never leave two versions writing the same profile. Next login alone does not run the server. |
| Mac sleeps and wakes | Recheck listener/session health and Keychain availability; do not infer that a timed-out TV write was not sent. |

An explicit Stop while the GUI is open must not be reversed by the GUI's
reconnect loop. Provide an in-app **Disconnect MCP / prepare to remove app**
path that explains and removes app-managed endpoint/auth state without
silently deleting TVs, trust records, or pairing tokens. Dragging an app
bundle to Trash does not run that cleanup; the plan must not promise an
automatic uninstall hook. A stale Claude Code entry should fail closed and
have a documented, token-free removal path.

## Secrets and migration

TV pairing tokens stay in the macOS Keychain and are accessed by the server
only after cutover. The existing `src/infrastructure/macos/keychain.rs` uses a
fixed generic-password service, so changing the preferences path does not
isolate development from production items. More importantly, existing items
were created by the GUI executable. A new helper's signature or default
Keychain access rules may not authorize it silently. Apple's file-based
Keychain documentation describes per-item ACLs and prompts for an untrusted
application; modern access-group sharing applies to macOS items only in the
data-protection/synchronizable cases. Do not assume that merely signing both
executables with the same certificate migrates legacy access:
[Keychain ACLs](https://developer.apple.com/documentation/security/access-control-lists),
[access groups](https://developer.apple.com/documentation/security/sharing-access-to-keychain-items-among-a-collection-of-apps).

### Silent credential access contract

Do not ask the end user to choose Keychain versus a database or present an
app-authored Keychain consent step. Explain the secure default in product
documentation. **Silent** means no routine Keychain prompt when starting or
attaching to the server, closing/reopening the GUI, connecting MCP, or reading,
saving, and deleting a paired TV token during normal unlocked-user operation.
It does not mean bypassing an OS trust decision, a locked Keychain, or TV-side
Pairing approval. The background server must use noninteractive credential
access: if macOS would require UI, return a typed blocked result and wait for
an explicit change in conditions or user action. Do not retry on a timer or
on each GUI/MCP reconnect, produce an unattended prompt loop, turn denial into
“no saved TV,” or silently re-pair.

Prototype a second `--server` mode of the *same signed app executable* as a
separate background process, alongside the distinct signed-helper design.
The former may preserve the trusted-app identity of GUI-created legacy items
without making the server part of the GUI process; it is not guaranteed by
using the same certificate or bundle path. It also does not provide OS-level
secret isolation between GUI and server modes. Choose only after signed
old-release-to-new-release tests establish the actual ACL and prompt behavior.
Keep stable signing and installation identity through upgrades; if neither
process shape accesses legacy items silently, specify a visible, one-time,
non-destructive recovery path instead of weakening storage.

Do not move tokens to a plaintext database or owner-only file merely to hide
Keychain prompts. An encrypted database whose key is in Keychain retains the
same access dependency; a key stored beside the database does not materially
protect the token from another process running as that user. Use persistent
files or a database for non-secret TV metadata and reconciliation state. A
future switch to macOS's data-protection Keychain requires its own user-context,
signing/entitlement, accessibility, and legacy-item migration tests; it is not
a flag change to assume safe or prompt-free:
[Mac Keychain implementations](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains).

P2-M6 must create a synthetic legacy item using the *old signed GUI* and test
read, update, and delete with both proposed *new signed server* process shapes,
including after an upgrade and with a locked Keychain. Count unexpected OS
prompts across repeated GUI and MCP launches; the normal path must have zero.
Prefer retaining the existing item and service if access works. If it does
not, design an explicit, backup-safe
migration and test its prompt behavior before the P2-M9 cutover. Do not export
TV tokens to JSON, command output, HTTP, logs, or fixtures; do not silently
erase old items or claim that re-pairing is unnecessary. A background server
must surface Keychain denied/locked/interaction-required as a typed blocked
state, never a false “no saved TV” or an unattended prompt loop. Confirm that
reconnect, retry, and server restart do not repeatedly invoke the blocked
Keychain operation without a relevant state change.

The GUI needs an API credential, not a TV pairing token. Store distinct GUI
and MCP client credentials with the server's signing identity in Keychain;
bootstrap the GUI through a protected, authenticated local channel without
an unauthenticated HTTP token endpoint. P2-M6 must fix the exact peer-identity
check and explain the same-user threat model. A private Unix socket with
`getpeereid` checks the account but not the application; a signed-client XPC
or audit-token/code-requirement check could additionally verify the GUI's
identity, but must be prototyped with the chosen signing method:
[Unix peer credentials](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/getpeereid.3.html),
[XPC peer requirements](https://developer.apple.com/documentation/updates/xpc).
A permissive same-UID channel or executable that prints a bearer token is
*not* a defense against malicious code already running as that user. The
Claude header helper also needs this explicit threat-model limitation;
automatic invocation is not authorization by itself. Rotation and
revocation must not strand the GUI or leak the prior credential. Development
uses fake credential ports or a separate Keychain namespace and never opens
production pairing items.

## Claude Code without token copying

Claude Code supports a user-scoped HTTP MCP configuration and a dynamic
`headersHelper` command that supplies headers on connect and reconnect:
[HTTP setup and scopes](https://code.claude.com/docs/en/mcp),
[dynamic headers](https://code.claude.com/docs/en/mcp#use-dynamic-headers-for-custom-authentication).
The proposed integration is a one-time **Connect Claude Code** action in the
GUI that uses Claude Code's supported user-scope configuration path, with the
HTTP URL and an absolute path to a bundled, signed auth helper but no bearer
value. It must preserve unrelated Claude configuration, detect same-name
entries at more-specific scopes, and never edit a project `.mcp.json` or
auto-approve a project. Unless intentionally stopped, the helper ensures the
server is running, retrieves only the MCP client credential, and emits the
expected JSON header to stdout. No TV token is
printed; no CLI MCP server or stdio transport is added. If Claude Code is
absent, show a copyable, token-free setup instruction rather than changing
unrelated files.

Claude Code may rerun the header helper and retry once after a `401` or `403`.
The server must reject authorization failures *before* admitting any effect,
and mutation operation keys remain required so a repeated call cannot cause
a second TV write within the retention window. Test client version and
configuration behavior; do not assume Claude Desktop supports this helper.
The first-time connection/approval to a third-party client remains an
intentional user choice, not a hidden grant of full TV control.

## Required system access and remaining user actions

The server requires outbound local-network access for Samsung WebSocket,
SSDP, and UDP Wake, plus a loopback listener for HTTP; the GUI needs loopback
HTTP only. On macOS 15 and later, Local Network privacy can prompt on the
first real TV access, and the permission is associated with responsible
signed code. Apple's
[TN3179](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy)
says a spawned helper can be attributed to its app, while a LaunchAgent needs
correct responsible-code association; test the actual signed packaging and
denied/revoked cases. Test both GUI-initiated and Claude Code-initiated
server launch: if macOS attributes the latter to Claude Code, the design
must change before claiming one durable TV-app permission.
`NSLocalNetworkUsageDescription` belongs in the responsible app's metadata.
`NSBonjourServices` is needed only if Bonjour
discovery is added; current SSDP is not Bonjour. macOS does not require the
multicast entitlement for this use. If App Sandbox is chosen, the server also
needs tested outgoing and incoming network entitlements (including UDP
behavior); do not add them blindly to an unsandboxed build. No root daemon,
Accessibility, Screen Recording, Input Monitoring, or Apple Events permission
is required for the stated architecture.

For outside-App-Store distribution, the current local self-signed bundle is
development evidence, not a prompt-free release artifact. Plan Developer ID
signing of each nested executable, hardened runtime, and notarization in the
release path, while preserving a local-signing development path:
[Apple notarization requirements](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).
Where applicable, Gatekeeper's initial launch confirmation, macOS Local
Network consent, any legacy Keychain ACL/unlock prompt, TV-side Pairing
approval, and one-time MCP client authorization cannot be bypassed honestly.
The plan's zero-action target is **no repeated setup or manual service
management after those explicit trust decisions**.
