# Software architecture

Status: Active

The default GUI uses the current in-process implementation. P2-M8 additionally
implements an explicit isolated development server and native GUI probe in the
same executable. The [Phase 2 target architecture](phase-2/target-architecture.md)
plans production GUI cutover in M9 and MCP in M10; those are not shipping yet.

The headless Tokio runtime in `application/runtime.rs` owns TV workflow,
effects and retained operations. The default Iced GUI calls it through the
temporary in-process adapter. Pairing/re-pair, reconnect rotation and Forget
use durable recovery markers; selection/configuration use fenced workers and
durable reconciliation. See [M7 evidence](phase-2/milestone-07-review.md) and
[M8 evidence](phase-2/milestone-08-review.md).

Samsung TV Remote is one Rust package. Its dependency direction is
`presentation → application → domain` and `infrastructure → application/domain`.
`main.rs` wires the adapters. UI code never constructs Samsung wire frames or
owns a socket; application policy never imports Iced or macOS APIs.

| Area | Responsibility |
| --- | --- |
| `src/domain/` | Device identity, MAC address, and finite semantic remote actions. |
| `src/application/` | TV selection, trust, pairing, connection, command admission and dispatch, Wake flow, typed outcomes, and ports. |
| `src/infrastructure/` | Secure Samsung session and codec, SSDP discovery, UDP Wake, preferences, and macOS Keychain. |
| `src/presentation/iced/` | Windows, input mapping, visual remote, view state, and user-facing feedback. |
| `src/server/` | Headless HTTP/SSE, access/bootstrap, detached lifecycle/launch and isolated fake-profile composition. |
| `src/control_client/` | Native bounded HTTP/SSE client, protected attach, reconnect policy and safe projections. |
| `src/main.rs` | Early same-executable mode dispatch, then default GUI composition. |

In explicit `--gui-probe --fake-profile` mode, Iced uses `control_client` over
loopback to a detached `--server` owner and constructs no local TV runtime.
The server uses owner-only `m8-fake-v1` storage, strict development-document
envelopes and synthetic credential presence markers. Production stores reject
that format even if renamed or copied. Startup performs no discovery, Pairing,
connect or Wake; all effects require explicit requests. GUI close detaches;
intentional Stop persists before acknowledgment and needs explicit GUI Resume.
The lifetime lock spans worker settlement and HTTP/bootstrap listener closure.

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

The Iced presentation maps pointer and keyboard input to typed service requests and
projects application results into the current [UX specification](ux-gui.md).
Sources, Apps, and Text Input remain placeholders. Native keyboard focus and
screen-reader support need further verification; see [Phase 1](phase-1/phase-1.md).

Use deterministic fakes for protocol, pairing, and network-boundary tests.
The [contribution guide](contribution-guide.md) defines validation. Historical
designs and milestone plans are in the [archive](archive/).
