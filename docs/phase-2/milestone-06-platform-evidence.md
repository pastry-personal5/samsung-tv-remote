# P2-M6 signed platform and client evidence

Status: Partial prototype, 2026-10-15. These observations do not prove a
production server, legacy Keychain migration, macOS privacy attribution, or
Claude Code compatibility.

## Credential scope update: 2026-11-12

The owner confirmed no device-key migration is needed, including synthetic
legacy migration. Earlier legacy migration/probe requirements below are
superseded by this decision. Signed fresh-credential Keychain behavior remains
M9 verification; existing items are not deleted or transferred by this scope
change. See the [current acceptance record](milestone-06-acceptance.md#owner-credential-scope-update-2026-11-12).

## Owner decision update: 2026-11-12

The owner selected on-demand background launch using the same signed app
executable in `--server` mode. The signed Keychain experiment remains open;
process selection does not establish legacy access or accept M6. Earlier
candidate comparisons below retain their research/historical context. See the
[current acceptance record](milestone-06-acceptance.md#owner-process-selection-2026-11-12).

## Human-run review: 2026-11-12

The [review record](milestone-06-review.md#human-run-review-2026-11-12)
documents corrections found while tracing the human instructions. Prepare
the required experiment with
`zsh scripts/stage_m6_human_handoff.sh --keychain-only`; a missing or invalid
optional LaunchAgent bundle no longer blocks it. The existing no-flag mode
still includes that comparison. The runbook keeps the handoff path available
for refresh, records both probe identities, distinguishes staging HEAD from
binary provenance, and explains recovery after partial destructive tests.

Validation delegated to the test runner on the current worktree:

| Check | Result |
| --- | --- |
| `zsh -n` on staging and both probe builders; all 17 runbook shell blocks | Passed; runbook blocks were parsed, not executed |
| Staging `--help` and invalid argument counts | Passed; invalid counts exit 2 |
| `cargo test --test m6_handoff --test m6_keychain_input` | Passed: 6 handoff and 2 input/preflight tests, using disposable fixtures and no Keychain requests |
| `cargo fmt --all -- --check`; `cargo clippy --all-targets -- -D warnings`; `git diff --check` | Passed |
| `cargo test` | 125 unit tests passed; existing Forget/recovery suite stopped the run with 4 failures and 3 passes. One failing result was `StorageFailed(Unavailable)` instead of completed removal. This review does not change that application/recovery work. |

This is a review of preparation and instructions. The isolated old-GUI
Keychain experiment, observed prompt/status results and process-choice
acceptance remain pending. No staging or automated fixture result closes
those gates.

## Human handoff refresh: 2026-11-12

The previously documented `/private/tmp/m6probe-bundles.66DVb8` directory was
absent. The dedicated `m6probe` account still exists without `admin` group
membership. Existing signed LaunchAgent, Keychain and old-GUI bundles passed
`codesign --verify --deep --strict` with normal host signing/trust access.
Sandbox verification failed because its signing/trust context was restricted;
no signing identity, certificate, trust setting or account was changed.

`zsh scripts/stage_m6_human_handoff.sh` prepares a fresh
`/private/tmp/m6probe-bundles.*` directory, prints its exact path, verifies
bundle identifiers and source/copy signatures, and includes `HANDOFF.txt`
plus the [human runbook](milestone-06-human-runbook.md) with that path filled
in and its three directly linked context records. The copied bundles and
instructions are readable by the test account and
have no group/world write permission. These are existing signed artifacts,
not a claim that the current worktree was rebuilt.

The test runner checked shell syntax, help, invalid/missing-input exits,
staging, copied signatures/identifiers, executable/read permissions, filled-in
path, `git diff --check`, and `cargo fmt --all -- --check`. No GUI or probe was
launched, agent registered, or legacy Keychain item accessed. The focused
human experiment remains pending and M6 stays Active.

## Completion attempt: 2026-11-11

Source revision `5d1a133` with the documented M6 plan revisions in the worktree.
Host: macOS 27.2.1 (26B131), arm64. Claude Code: 2.1.322. The signed probes use
the existing Samsung TV Remote Local Test identity, isolated `target/`
profiles, and synthetic loopback traffic. No TV pairing credential or real
TV address was used. The commands required normal host signing/socket access;
sandbox-only attempts could not access the signing identity or bind sockets.

| Command | Expected / actual result |
| --- | --- |
| `zsh scripts/run_m6_signed_lifecycle_probe.sh` | Passed signature verification; same-executable and helper children survive parent exit; duplicate and unrelated-listener collisions rejected; durable owner-only Stop respected, explicit Resume succeeds, forced crash permits later launch. No TV-network or Keychain token code is exercised. |
| `python3 scripts/probe_claude_mcp_revision.py` | CLI config and health check both exited zero. Observed authenticated fake `server/discover` and `tools/list`, with matching `2026-07-28` headers and body metadata. No model workflow or product server tested. |
| `python3 scripts/measure_mcp_corpus.py` | Reproduced the recorded 13-tool, 1,606-unit catalog plus 83-unit instruction and all seven workflow proxy totals. This remains a byte-based estimate, not actual model tokens. |
| `zsh scripts/build_m6_signed_keychain_probe.sh` | Compiled and deep-verified both signed executables. Neither accessed a Keychain item; rebuilding is not migration acceptance. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets -- -D warnings` | Passed. |
| `cargo test` | Passed: 124 unit and 17 integration tests. |

The automated commands returned without requesting an interactive approval.
This does not measure GUI/MCP routine Keychain prompts: no legacy token was
read. The isolated-desktop synthetic legacy-item experiment remains unrun
unless the owner supplies its results. M6 stays Active; do not substitute
P1-M13 hardware acceptance for this migration test. Lifecycle and endpoint
feasibility are accepted for design freeze. Final process identity remains
conditional on Keychain evidence and the M9 real-identity cutover test.

## Environment and completed experiment

- Host: macOS 26.7.1, arm64. The installed Claude Code CLI reports 2.1.300.
- The existing signed app bundle verifies with `codesign --verify --strict`.
  Its bundle ID is `dev.samsungtvremote.local`, signed by the local test
  identity. Its Mach-O `LC_BUILD_VERSION` says minimum macOS 11.0 and SDK
  26.1. **This is deployment metadata, not tested macOS 11 support**. Phase 2
  targets the owner’s Mac and a local signed installation; no
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
never token contents. It can read one exact legacy service/device record ID; its
replace-and-delete mode requires both `M6_ISOLATED_KEYCHAIN_ACCOUNT` to match
the logged-in test user and `M6_SYNTHETIC_DEVICE_ID` to match the exact
`dev_<decimal>` account. It replaces the fake TV token with a fixed synthetic
value before
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
The signed probe above supports continuing this candidate, but M6 must
record a conditional process choice until focused Keychain feasibility is
resolved; actual final-identity access is a mandatory M9 cutover gate.
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

## Original gate inventory and updated ownership

The 2026-11-11 [acceptance and handoff record](milestone-06-acceptance.md)
assigns focused feasibility probes to M6 and production tests to M7–M11.
The inventory below remains useful, but is not an all-M6 completion checklist.
LaunchAgent registration is optional rejected-alternative research. One
explicit user-assisted migration approval is allowed; routine access must
remain prompt-free. Public distribution/notarization is outside Phase 2.

The [isolated-account human runbook](milestone-06-human-runbook.md) gives the
commands for the prepared signed probes and the stop points for unexpected
results.

| Gate | Required evidence |
| --- | --- |
| Legacy Keychain | In an isolated macOS test account, create a synthetic item with the **old signed GUI** and a fake TV. The old binary hardcodes the production service name, so a separate test account is required to avoid touching this user's production Keychain. Run both signed paths of the prepared `m6-keychain-probe` on that exact saved device ID: read first, then deliberately replace/delete the synthetic item and verify its absence. Record access prompts and status; repeat for locked, denied, interaction-required, repeated GUI/MCP starts and an upgrade. The prepared probe has a new bundle identity and does not prove access from the eventual same-executable server mode; that path must be tested when implemented. It does not implement rollback. |
| Local Network privacy | Signed GUI-initiated and Claude-initiated launches on macOS 15+, actual TV-network attempt, attribution, denial/revocation and repeat-launch prompt count. A fake TV can cover protocol behavior but cannot prove the OS consent attribution alone. |
| Endpoint | Stage the prepared signed LaunchAgent bundle in an isolated account and resolve the workspace `status: notFound` result before testing immediate registration, login and unregister behavior. Test stale manifest, two simultaneous client launches, real-operation crash recovery, upgrade drain, logout, fast user switching and active Claude session after reconfiguration. The detached probe covered an unrelated listener's occupied port, parent exit, marker-based Stop/Resume and forced-crash relaunch only. |
| Client | Run a real Claude Code connection against signed `/mcp`; test operation results, tool search on/off, 401 refresh, model-visible catalog/results and workflow turns. The isolated fake endpoint measured CLI health-check negotiation only. |
| Security and rollback | Verify Host/Origin, authentication of all reads/writes, symlink-safe socket and manifest creation, credential rotation/revocation, no routine prompts, safe old-profile backup and rollback. |

The existing plist inventories `NSLocalNetworkUsageDescription`; current
SSDP does not use Bonjour, so there is no `NSBonjourServices` entry. The
current bundle is unsandboxed and has no sandbox network entitlements. M9
must sign every nested executable with a stable local identity for the owner’s
Mac. Public distribution and notarization are outside Phase 2 acceptance.
The local signature does not prove public distribution behavior.

## 2026-11-12 human-test input correction

Human execution reported usage output and exit 2 on the read command. The
probe incorrectly required a formatted UUID, while `DeviceId::Display` and
`KeychainCredentialStore` use the exact `dev_<decimal u128>` record key.
The probe now validates that saved format and uses
`M6_SYNTHETIC_DEVICE_ID` for its exact-match isolation guard. A
`validate-input` command checks both guards without accessing Keychain.

The new `tests/m6_keychain_input.rs` compiles the Swift probe and checks
fixed saved IDs, u128 bounds, malformed IDs, and isolation mismatches
without reading any credential. Formatting, Clippy, and the full Cargo test
suite passed. The corrected signed bundle was rebuilt and deep-verified.
Actual Keychain feasibility remains pending the isolated human rerun.

Review follow-up: the runbook now chains preflight and read so failed input
validation cannot proceed to a Keychain request. A fake-probe test executes
that command block and verifies the failure exit is reported without a read.
The Swift boundary tests now use fixed IDs and cover all three operations,
missing/incorrect isolation variables, invalid command shapes, non-ASCII
input, and numeric overflow. The obsolete `M6_SYNTHETIC_DEVICE_UUID` variable
produces an explicit migration diagnostic without relaxing the exact-ID guard.
The refresh instructions also check the isolated account before copying.
