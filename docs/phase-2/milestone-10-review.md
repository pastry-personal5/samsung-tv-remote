# P2-M10 acceptance review — 2026-11-14

Status: Active; not accepted as Done.

The owner requested completion and confirmed that a disposable macOS account
and a separate controlled LAN fixture are unavailable for this run. Local
implementation and isolated validation proceed; production-helper, native
attribution and actual-model budgets are not waived or transferred to M11.
A later 2026-11-14 [owner decision](phase-2.md#owner-decisions) waived the
production-helper and separate-host lanes (not the model budgets).
Existing Phase 3 and M11 planning edits are preserved.

## Delivered in this pass

- Typed native-client connection management, explicit GUI Check/Connect,
  separate endpoint verification, confirmed all-session Disconnect, confirmed
  changed-helper/relocation repair, acknowledged partial progress and retry
  using the original generation/key/expected entry.
- Conservative client/scope preflight: bounded three-second version probe,
  supported 2.1.325 writer/helper-entry compatibility, known local/project and
  ancestor conflicts, managed/restrictive files and unsafe configuration.
  A development probe needs an explicit isolated `CLAUDE_CONFIG_DIR`.
- An authenticated app-side `/mcp` catalog verification read, with strict
  loopback destination and no redirect following. Success is not labeled as
  actual Claude attachment. Intentional Stop remains respected.
- Discovery/probe inspection of an already available private terminal receipt,
  without waiting, allocating a subscriber or resubmitting an effect.
- Shipped-schema validation of all 15 corpus inputs; fake-port coverage for
  delayed/retained observations and changed-certificate re-pair confirmation.

## Overview checklist audit

| Requirement | Implemented evidence | Remaining acceptance |
| --- | --- | --- |
| Revision, framing, metadata, errors and direct first call | `mcp_protocol`, shared-listener `control_http` fixtures, strict native verification headers | Real installed-client revision negotiation. |
| Shared limits, classification and cancellation | Existing mutation saturation, bounded wait/subscriber and disconnect fixtures | Complete mixed-client acceptance review; no extra pool allowance introduced here. |
| Full service parity | All 15 tools map to the shared service; first-call workflow, recovery markers, re-pair and native parser/schema corpus tests | Signed client all-tool/recovery parity. |
| Explicit Pair/Forget confirmation | Matching fingerprint/target guards; new changed-certificate rejection before Pairing | Actual client's human-confirmation behavior. |
| Delivery uncertainty and no replay | Existing cross-route original-key/private lookup and one-write/Wake tests; GUI connection retries retain their management key | Installed-client disconnect/refresh evidence. |
| Stable semantic catalog and annotations | Existing deterministic catalog/protocol tests | Actual-client annotation/replay behavior and model-visible definitions. |
| Bounded summaries/detail/recovery/expiry | Existing paging/orphan-recovery/schema tests; new retained observation regression | Workflow projection/cost reconciliation with actual client. |
| Cache, output channels, calls and tokens | Both result channels validated; all corpus inputs match shipped schemas | Nine workflows in both search modes, actual visible tokens/calls/cache conditions and justified budget exceptions. |
| User entry, helper, startup/Stop, identity and destination | Existing helper/offline/server-process fixtures; new preflight and app-side catalog read | Production signed helper, refresh/rotation/termination/hostile-peer client checks and Claude-launched native attribution. |
| Separate connection facts, durable disable, repair/races | Ordered coordinator, original-key recovery and exact-entry writer tests; new typed GUI/repair confirmation tests; isolated actual CLI directory-lock interoperability | Full publication/crash audit and signed native connection/repair review. |
| Installed production smoke and acceptance topology | Development fake-port and isolated client checks only | Disposable-account production helper/client mutation; signed isolated model corpus; separate-host Local Network fixture. |

Each row retains its required unverified component. The overview boxes are not
checked merely because a deterministic fixture exists.

## Client and budget evidence

Installed Claude Code **2.1.325** accepted a token-free `type:http`, loopback URL,
`headersHelper` user entry in a fresh `/private/tmp` `CLAUDE_CONFIG_DIR`. Holding
its sibling `.claude.json.lock` directory blocked the CLI writer until release.
No owner configuration or credential was changed. This supersedes the earlier
2.1.322 writer observation for the new version-pinned preflight.

An actual CLI health check then used a fresh fake profile and a fresh isolated
Claude configuration. Protected `--mcp-management prepare` allocated the stable
endpoint, followed by explicit `enable 0 <fresh UUID>`. The token-free entry
used the absolute built executable with `--mcp-headers --fake-profile` as its
helper. With the GUI closed and server initially absent, both
`claude mcp get samsung-tv-remote` and `claude mcp list` exited zero and reported
**Connected**. The helper cold-started the actual development server. Explicit
`--server-stop --fake-profile` then exited zero with durable Stop confirmation.
This is actual-client development endpoint/helper evidence, not signed
production identity, Keychain, model tool execution or Local Network evidence.

`every_corpus_input_conforms_to_the_shipped_tool_schema` reports the complete
shipped catalog at **57,345 UTF-8 bytes / 14,337 proxy units**, plus **275 bytes /
69 proxy units** for instructions. These counts include output schemas and
annotations. The historical M6 catalog estimate describes different proposed
definitions; it cannot accept the implemented catalog. The byte/4 proxy is not
an actual model token count, nor proof of which definition fields Claude sends
to the model. Actual client-visible definitions and both result channels must
be captured before accepting or optimizing against the 1,900-token gate.

## Validation and unresolved reliability

The delegated full `cargo test` run passed **399 tests, zero failed**, including
195 unit tests and all 18 server-process cases. Two native-client regressions
added after that run began then passed focused execution: malformed-file durable
revocation and trusted-endpoint relocation repair. Latest-tree
`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` and
`git diff --check` also passed. The
full run used the documented isolated loopback/process permissions; captured
Clippy and test exit statuses were both zero. The shipped-schema corpus
measurement test also passed separately with `--nocapture`.

The debug renderer produced a [Settings → Servers capture](evidence/m10-claude-settings.png)
using a fresh fake profile and isolated Claude directory. Visual inspection
confirmed readable controls without clipping. This proves the initial panel's
rendering, not production signing, accessibility or client attachment.

A fresh isolated `claude auth status` returned `loggedIn:false` and
`authMethod:none`. No credentials were copied into that configuration; actual
model workflows and token/call measurements therefore remain unavailable.

The earlier full parallel run reproduced
`admitted_management_retains_profile_ownership_after_composition_guard_drops`;
ten focused reruns passed. The assertion now reports its non-secret file error
if it recurs. No root cause or reliability fix is inferred from focused passes.

## Handoff

Follow [connection guidance](milestone-10-connection.md) and the frozen
[acceptance topology](milestone-10-architecture.md#acceptance-topology). M11 may
prepare independent integration fixtures, but its predecessor matrix must keep
M10 signed-helper/client/native and model-budget requirements open. M10 remains
Active until every required lane has observed evidence.
