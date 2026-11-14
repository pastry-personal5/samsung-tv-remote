# Software architecture

Status: Active

The default GUI is a client of the Control API Server over protected loopback
HTTP/SSE. The same signed executable dispatches server/management modes before
Iced starts. P2-M9's source cutover is implemented; its signed production
acceptance remains open. See the [implementation ledger](phase-2/milestone-09-implementation.md)
for the failed self-signed rebuild gate and current native evidence. MCP remains
P2-M10 work in the [target architecture](phase-2/target-architecture.md).
The shared listener now implements `/mcp`, with the same service admission,
private operation history and limits as `/v1`. Native connection controls use
`control_client/mcp.rs`; presentation receives typed configuration/enrollment
facts and never edits Claude files or enrollment records itself. Client/scope
preflight precedes explicit Connect, while endpoint verification is a separate
authenticated catalog read. Installed-client and model-budget acceptance remain
open in the [M10 review](phase-2/milestone-10-review.md).

The headless Tokio runtime in `application/runtime.rs` owns TV workflow,
effects and retained operations in the server process. Iced owns local windows,
drafts and intent context and consumes typed API results. It constructs no local
runtime, TV or persistence adapter. Pairing/re-pair, reconnect rotation and Forget
use durable recovery markers; selection/configuration use fenced workers and
durable reconciliation. See [M7 evidence](phase-2/milestone-07-review.md) and
[M8 evidence](phase-2/milestone-08-review.md).

Samsung TV Remote is one Rust package. Server composition connects
`application → domain` policy to `infrastructure → application/domain` adapters.
Presentation uses `control_client` and pure shared values. `main.rs` dispatches
modes and composes the client; `server/process.rs` composes production adapters.
Before dispatch, bundled entrypoints retain a shared executable-inode lock.
The installer exclusively locks old/staged executables through replacement and
rollback; concurrent launches fail before opening profiles. Legacy upgrade
handoff also uses post-quarantine process checks. This publication guard owns
no TV policy, storage schema or removal state; see the
[M11 ledger](phase-2/milestone-11-implementation.md).
UI code never constructs Samsung wire frames or owns a TV socket; application
policy never imports Iced or macOS APIs. The required GUI boundary check rejects
runtime/adapters, raw service receipts and persistence access from presentation.

| Area | Responsibility |
| --- | --- |
| `src/domain/` | Device identity, MAC address, and finite semantic remote actions. |
| `src/application/` | TV selection, trust, pairing, connection, command admission and dispatch, Wake flow, typed outcomes, and ports. |
| `src/infrastructure/` | Secure Samsung session and codec, SSDP discovery, UDP Wake, preferences, and macOS Keychain. |
| `src/presentation/iced/` | Windows, input mapping, visual remote, view state, and user-facing feedback. |
| `src/server/` | Headless HTTP/SSE, access/bootstrap, detached lifecycle/launch and production/fake composition. |
| `src/control_client/` | Native bounded HTTP/SSE client, protected attach, reconnect policy and safe projections. |
| `src/main.rs` | Early same-executable mode dispatch, then default GUI composition. |

Default startup authenticates code identity before opening the fresh
`control-v1` profile. The server exclusively owns fresh pairing/API Keychain
services; legacy data is left untouched. Explicit `--gui-probe --fake-profile`
uses the same client with separate owner-only `m8-fake-v1` storage, strict
development-document envelopes and synthetic credential presence markers.
Production stores reject that format even if renamed or copied.

Exceptional API-item access can be requested through the actual Apple-signed
executable's explicit `--server-api-access --request-access` maintenance mode.
It runs before GUI/runtime/profile construction and reads only the existing
fresh-service API roles, without printing or changing their values. Its
`--inspect` variant makes no Keychain calls. Neither ordinary startup nor
credential recheck invokes this foreground approval path; routine server
credential queries keep Keychain UI disabled. Approving a standalone helper
does not prove access for the application's executable path.

Server startup performs no TV action. After authenticated restore, each GUI
lifetime may request one Connect for an idle, disconnected saved selection.
An atomic server guard rejects busy/ready races without retry. GUI close detaches;
intentional Stop persists before acknowledgment and needs explicit GUI Resume.
The lifetime lock spans worker settlement and HTTP/bootstrap listener closure.
An incompatible owner needs explicit restart. Attachment recovery uses one
bounded tracker and lookup scheduler and never replays a TV mutation.

The private runtime owner holds the Selected TV, selection generation,
connection lifecycle, and one bounded command queue. Each request targets a
specific saved device and returns a typed rejection or transport outcome.
The Samsung session serializes writes on the secure port-8002 WebSocket.
A successful socket write means a command was sent; it does not prove that
power, volume, or another TV state changed. Do not replay uncertain actions
automatically.

Discovery and manual entry yield untrusted candidates. Check TV probes the
secure endpoint and presents its observed certificate identity. Pairing
requires confirmation of the intended TV and approval on that TV. Pairing
tokens belong in Keychain; device preferences and certificate trust are stored
separately. A changed certificate fails the saved trust check before sending a
token. Selection, pairing, and a live connection are separate facts.

Wake uses a selected TV's configured wired or Wi-Fi MAC address. It sends one
local-route magic packet, then waits for a usable paired remote connection.
Packet transmission does not prove delivery or visible panel power. A live
paired Power Toggle sends `KEY_POWER`. The visual remote's Play/Pause intent
resolves to `KEY_PLAY` when playback is unknown or the app last requested a
pause, and to `KEY_PAUSE` after a confirmed Play write. The Samsung remote
channel does not report independent playback state, so a selected or
reconnected TV starts unknown. A socket write records only the app's inferred
next state; it does not observe the player.

The Iced presentation maps pointer and keyboard input to typed API requests and
projects application results into the current [UX specification](ux-gui.md).
Sources, Apps, and Text Input remain placeholders. Native keyboard focus and
screen-reader support need further verification; see [Phase 1](phase-1/phase-1.md).

Use deterministic fakes for protocol, pairing, and network-boundary tests.
The [contribution guide](contribution-guide.md) defines validation. Historical
designs and milestone plans are in the [archive](archive/).
