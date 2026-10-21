# P2-M6 signed platform and client evidence

Status: Partial prototype, 2026-10-15. These observations do not prove a
production server, legacy Keychain migration, macOS privacy attribution, or
Claude Code compatibility.

## Environment and completed experiment

- Host: macOS 26.7.1, arm64. The installed Claude Code CLI reports 2.1.300.
- The existing signed app bundle verifies with `codesign --verify --strict`.
  Its bundle ID is `dev.samsungtvremote.local`, signed by the local test
  identity. Its Mach-O `LC_BUILD_VERSION` says minimum macOS 11.0 and SDK
  26.1. **Minimum supported macOS remains 11.0 for this plan**; no
  `SMAppService` dependency may be introduced without changing that support
  decision and testing an older system. This binary metadata alone does not
  prove runtime behavior on macOS 11.
- `zsh scripts/run_m6_signed_lifecycle_probe.sh` built a separate, isolated
  signed bundle with the same test identity and passed deep signature
  verification. Its launcher exited after
  spawning a server mode of the same executable, then after spawning a
  separately signed helper. Both remained reachable on `127.0.0.1` after
  launcher exit and both rejected a second bind on the occupied port. The
  signed server also rejected a port held by an unrelated Python listener.
  In the same isolated profile, a Stop request wrote and synced an owner-only
  marker before acknowledgment, the launcher refused to restart it, and an
  explicit Resume cleared it and relaunched. A forced crash left no Stop
  marker, so the second launch could proceed.
  The script killed each child before the next case. It used no real profile,
  credentials, TV network, or login registration.

This establishes only that a detached signed child can survive its parent
under this test session and that a kernel bind prevents duplicate use of one
port. Its marker test proves only isolated launcher behavior; it does not
prove control-operation drain or recovery, logout, an app upgrade, fast user
switching, macOS Local Network privacy, or the old GUI's Keychain access. The
probe's unauthenticated response contains only test constants and is not the
proposed server design.

`python3 scripts/probe_claude_mcp_revision.py` used a temporary home and
Claude configuration, a loopback-only fake MCP endpoint, and a fake
`headersHelper` credential. `claude mcp get` sent `server/discover` and
`tools/list` with `2026-07-28` in both the HTTP header and request `_meta`;
both carried the fake auth header. In `--legacy` mode, the probe rejected
modern discovery and the CLI fell back to `initialize`, a 2025-11-25
notification, GET, and `tools/list`. This records actual CLI health-check
negotiation. It is not a model workflow or a signed product-server test.

`zsh scripts/build_m6_signed_keychain_probe.sh` also built and deep-verified
an isolated app bundle containing a signed main executable and a separately
signed helper. Both use a new probe bundle identity; neither is the old GUI
or the eventual same-executable server mode. **Neither executable was run
against Keychain.** The probe
uses a noninteractive `LAContext` and reports status codes and byte counts,
never token contents. It can read one exact legacy service/device UUID; its
replace-and-delete mode requires both `M6_ISOLATED_KEYCHAIN_ACCOUNT` to match
the logged-in test user and `M6_SYNTHETIC_DEVICE_UUID` to match the account
UUID. It replaces the fake TV token with a fixed synthetic value before
deleting it, so that mode is destructive and belongs only in the isolated
test account. Apple's [`LAContext.interactionNotAllowed` guidance](https://developer.apple.com/documentation/localauthentication/lacontext/interactionnotallowed)
and [`kSecUseAuthenticationUIFail` guidance](https://developer.apple.com/documentation/security/ksecuseauthenticationuifail)
explain the noninteractive Keychain path. Compilation and signing are only
preparation, not evidence of prompt behavior or migration compatibility.

`zsh scripts/build_m6_signed_launch_agent_probe.sh` built a second isolated
bundle with a signed one-shot helper, an app-bundled LaunchAgent plist, and a
signed `SMAppService` controller. `plutil` accepted both plists and deep code
signature verification passed. The helper would append only a timestamp and
PID to `~/Library/Application Support/M6LaunchAgentProbe/starts.log` in the
account where it runs. **The agent was not registered or launched.** A
read-only `status` call against this bundle while it sat under `target`
returned `notFound`, even after placing the helper in Apple's documented
`Contents/Resources` location. The controller reported the expected main
bundle ID and confirmed that the agent plist exists at its bundle URL, so
the result is not explained by a missing plist in `Bundle.main`. A deep
signature-verified copy under `/private/tmp` also returned `notFound`, so the
external workspace volume alone does not explain it. Neither copy was
registered. Apple's
[`notFound` definition](https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.enum/notfound)
means the framework could not find the service; the build and signature
checks do not establish that `SMAppService` recognizes this bundle as
installed. The next comparison must first stage the bundle in the isolated
account and obtain a meaningful status, then test immediate registration,
later login, unregister, and logout behavior. Apple's [app-bundled service layout](https://developer.apple.com/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos)
specifies the `Contents/Library/LaunchAgents` plist and bundle-relative
`BundleProgram` used here.

## Candidate lifecycle and access decision

The preferred implementation candidate is on-demand detached server launch.
An app-bundled LaunchAgent is not selected for the no-login-start product
rule. Apple's [`SMAppService.register()` documentation](https://developer.apple.com/documentation/servicemanagement/smappservice/register%28%29)
states that a registered LaunchAgent is bootstrapped immediately and on each
subsequent login; the API is available only on macOS 13 and later. The signed
LaunchAgent bundle has been built but its registration comparison has not
been run. It must use an isolated test account
so registration and its login behavior do not change the current user's
session.
The signed probe above supports continuing this candidate, but M6 cannot
freeze the process shape until the remaining signed tests below are run.
Closing the GUI detaches only its client; explicit Stop writes an owner-only
durable marker before acknowledging, drains or marks operations uncertain,
and prevents GUI or MCP helpers from restarting the process. A crash does not
write Stop and a later explicit client connection may start a new epoch.
Only an explicit signed-GUI Resume action clears the sticky marker. With no
server listening, the GUI launcher takes the per-user lifecycle lock,
validates the owner-only marker, clears it atomically, and starts or attaches.
The MCP header helper has no Resume capability. A lost Stop HTTP response is
resolved from marker state and authenticated readiness without restarting.

The intended stable endpoint is a random high loopback port allocated once
per macOS user, saved in an owner-only manifest under Application Support.
The server takes an owner-only single-instance lock before binding and
publishes the manifest only after authenticated readiness. A port collision
fails closed, including when the occupant is unrelated; no process is killed
and no alternative URL is silently selected. An app-managed reconfiguration
may update the saved URL for a future Claude session, but it cannot retarget
an already-running HTTP client connection. Fast user switching remains a
specific test: different users can collide on the same TCP port despite
separate manifests.

The credential bootstrap candidate is an owner-only Unix socket in a 0700
per-user directory. `getpeereid` must match the current effective UID and
the socket and manifest are never symlinks. Only after that check may the
server issue a GUI client credential or, after an explicit Connect Claude
Code action, a separate MCP client credential. Credential material stays in
Keychain; HTTP never returns it from an unauthenticated route. Requests,
readiness and events require a bearer credential, and the server maps each
to its own stable logical client identity. Rotation issues a new credential,
switches the accepted generation atomically, then revokes the old one;
revocation blocks new calls before effects. Keychain denial, lock or required
UI is a typed blocked state, never a silent fallback.

The HTTP listener binds only `127.0.0.1`, never `0.0.0.0` or IPv6 in this
phase. The access gate accepts only `Host: 127.0.0.1:<manifest-port>`; it
rejects DNS aliases and Host mismatches before a route handler. Native GUI
and Claude requests may omit `Origin`. If an Origin is present, it must be
exactly `http://127.0.0.1:<manifest-port>`; `null`, other ports, and other
origins are rejected with no CORS wildcard. Authentication applies even to
safe reads and the event stream. The manifest is not proof of listener
identity: attachment requires an authenticated readiness challenge to the
expected API/build and epoch.

This is deliberately a **same-user trust boundary**. `getpeereid` proves a
Unix account, not a signed app identity. Code already running as the same
user could invoke the bootstrap or bundled auth-header helper and impersonate
an authorized local client. The local mechanism defends against remote and
other-user access, browser-origin attacks and accidental token disclosure;
it does not claim protection against a malicious same-user process. A
stronger signed-client audit-token or XPC check can be evaluated in M8, but
must be prototyped rather than claimed now. If that stronger boundary becomes
a product requirement, this candidate needs revision before release.

The MCP endpoint uses a restricted local bearer mode rather than advertising
the optional OAuth authorization profile. The Connect Claude Code action
provisions a user-scope HTTP entry with a stable URL and an absolute path to
a signed `headersHelper`; it does not put a static token in configuration.
The helper may emit only the MCP `Authorization` header JSON to its caller,
never a TV pairing token. A same-user process can run it, consistent with
the stated threat boundary. Header refresh/retry must be rejected before
command admission when unauthorized. The installed CLI and published Claude
Code documentation support an HTTP MCP entry and `headersHelper`. The
isolated CLI revision and fake helper behavior are measured above; real app
credential storage, reconnection and tool behavior remain unmeasured.

## Gates still required before M6 can be marked done

The [isolated-account human runbook](milestone-06-human-runbook.md) gives the
commands for the prepared signed probes and the stop points for unexpected
results.

| Gate | Required evidence |
| --- | --- |
| Legacy Keychain | In an isolated macOS test account, create a synthetic item with the **old signed GUI** and a fake TV. The old binary hardcodes the production service name, so a separate test account is required to avoid touching this user's production Keychain. Run both signed paths of the prepared `m6-keychain-probe` on that exact UUID: read first, then deliberately replace/delete the synthetic item and verify its absence. Record access prompts and status; repeat for locked, denied, interaction-required, repeated GUI/MCP starts and an upgrade. The prepared probe has a new bundle identity and does not prove access from the eventual same-executable server mode; that path must be tested when implemented. It does not implement rollback. |
| Local Network privacy | Signed GUI-initiated and Claude-initiated launches on macOS 15+, actual TV-network attempt, attribution, denial/revocation and repeat-launch prompt count. A fake TV can cover protocol behavior but cannot prove the OS consent attribution alone. |
| Endpoint | Stage the prepared signed LaunchAgent bundle in an isolated account and resolve the workspace `status: notFound` result before testing immediate registration, login and unregister behavior. Test stale manifest, two simultaneous client launches, real-operation crash recovery, upgrade drain, logout, fast user switching and active Claude session after reconfiguration. The detached probe covered an unrelated listener's occupied port, parent exit, marker-based Stop/Resume and forced-crash relaunch only. |
| Client | Run a real Claude Code connection against signed `/mcp`; test operation results, tool search on/off, 401 refresh, model-visible catalog/results and workflow turns. The isolated fake endpoint measured CLI health-check negotiation only. |
| Security and rollback | Verify Host/Origin, authentication of all reads/writes, symlink-safe socket and manifest creation, credential rotation/revocation, no routine prompts, safe old-profile backup and rollback. |

The existing plist inventories `NSLocalNetworkUsageDescription`; current
SSDP does not use Bonjour, so there is no `NSBonjourServices` entry. The
current bundle is unsandboxed and has no sandbox network entitlements. M9
must sign every nested executable with a stable release identity and review
Developer ID/hardened runtime/notarization for distribution. The local test
signature is development evidence only.
