# P2-M8 completion review

Status: **Done**, 2026-11-12. This review maps every
[overview checklist item](milestone-08-overview.md#completion-checklist) in order.
M8's three required deliveries are implemented: the complete fake-profile API,
runnable detached lifecycle and early existing-GUI client. Production signed
identity/Keychain and full GUI cutover remain the explicitly assigned M9 scope.

## Follow-up code review, 2026-11-12

Four P2 client reliability findings were corrected:

- Cancelling `Remote::submit` after sending left its intent `InFlight`, excluded
  from recovery and permanently consuming a tracking slot. A drop guard now
  marks it uncertain and preserves its original epoch/key for lookup only.
- Cancelling an SSE read reset its timeout; a silent stream could stay connected
  indefinitely. The idle deadline now belongs to `Events` and survives cancelled
  reads. Incoming heartbeats refresh it without creating application events.
- Sequential result lookups ran inside the event-selection branch. At the
  64-intent limit, five-second HTTP timeouts could defer SSE processing for
  minutes. Recovery now runs concurrently with event consumption and is dropped
  when the subscription ends; no mutation is replayed.
- A subscription rejected with HTTP 429/503 was classified as invalid wire data,
  permanently blocking reconnect. These capacity/drain responses now retry under
  the existing bounded backoff; invalid responses still block.

New regressions exercise actual submission cancellation against a silent
loopback peer and Hyper event bodies over in-memory duplex transports. Paused
clock tests prove repeated read cancellation still times out at 35 seconds and
heartbeats extend liveness until 35 seconds after the last received heartbeat.
Subscription fixtures distinguish retryable 429/503 from authentication and
malformed-response failures. Existing GUI rotation/process fixtures cover the
concurrent recovery composition and lookup-only recovery behavior.

Follow-up validation passed: `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings` and the full `cargo test`, including
all four new regressions and the existing HTTP/rotation/process fixtures. The
original captured validation below predates this follow-up.

## Requirement-by-requirement evidence

| Item | Authoritative evidence |
| --- | --- |
| 1. Same-executable headless/fresh profile | `server_process::executable_modes_select_the_gui_probe_before_any_local_adapter` and detached lifetime/profile restart fixture; three `development_profile` cases prove empty/no legacy import, zero startup credential/TV effects, isolated restart and rejected invalid envelopes. `main` dispatches before Iced. |
| 2. Every route/outcome over HTTP | All scoped routes enter `ControlService`. The 26 `control_http` scenarios cover setup/re-pair, selection/connect, configuration, remote/Power/Wake/cancel, Forget and both recoveries. They exercise every session/discovery/setup/reconnect failure, actual journal/store failures and partial cleanup, policy/overload, late cancellation, deadline, supervision, drain and uncertainty. Six `control_response` fixtures cover every top-level/nested typed HTTP projection; exhaustive Rust matches reject unmapped future variants. See the detailed coverage below. |
| 3. Shared state, admission/dedup/uncertainty | Actual two-client HTTP/SSE fixtures, changed/duplicate keys, response loss, stale epoch/generation, private key lookup and remote FIFO overload. M7 owner/registry fixtures additionally prove retention, per-client quotas and duplicate admission at capacity. No lookup or retry repeats an effect. |
| 4. Ingress/access policy | Actual raw CL+TE/duplicate framing, Host/Origin/proxy/header/body/auth/capability fixtures; `server_access` tests literal peer/authority policy and bounded failures. Protected readiness uses the same gate, with a restricted one-use bootstrap capability. |
| 5. Atomic ordered private SSE | Two actual clients receive atomic snapshots and consecutive revisions, without private request/results. Rotation closes old streams; overflow unit fixture discards buffered events and emits resync/EOF. Actual maximum profiles stream to all eight reachable subscribers; subscriber limits preserve queries. |
| 6. Detached owner/endpoint/readiness | Eleven `server_process` scenarios prove simultaneous launch/same owner, parent/no-client survival, stable endpoint, occupied/corrupt metadata rejection, protected attach and explicit incompatible restart. The minimal child verifies a new Darwin session, exact inherited descriptors, `/dev/null` stdio and `PATH`-only environment. |
| 7. Durable Stop/Resume/settlement | Actual sticky Stop, explicit Resume and lost-response observation; the native launcher preserves definite persistence failures and rejects acknowledgments with the wrong epoch/key; lifecycle fixtures fence admission before persistence, retain failed/lost acknowledgments and wait for late workers. Ownership remains in the completed actor join result until composition closes HTTP/bootstrap listeners. Signals do not create sticky Stop. |
| 8. Revocation ordering/rotation | Queued authorization is rechecked under the owner admission guard; access/HTTP fixtures revoke active streams. `control_client_rotation` drives the actual GUI actor through fake setup/action, API-secret rotation, a fresh same-owner snapshot and revocation block. Probe/approval/connect/write counts stay exactly one. Challenges invalidated by rotation require a fresh exchange; revocation remains denied. |
| 9. Lifecycle race/crash matrix | Actual Stop/start race, explicit incompatible restart, crash/new epoch, Resume waiting for held ownership and termination without Stop. `private_files` subprocesses abruptly exit the actual writer after creation, write, file sync, rename and directory sync, with absent and existing markers. Actual `FileStopStore` reads only complete old/new authority; the lock is reacquirable and temporary entries are ignored. Lifecycle tests separately prove acknowledgment follows successful persistence and late-worker settlement. |
| 10. Protected runtime/bootstrap | Owner UID/modes/no-follow/link checks, atomic replacement and concurrent reads; nine bootstrap scenarios and [transcript review](milestone-08-bootstrap.md). Public Unix socket appears only after private staging/mode validation and cannot replace a live socket. Fake same-UID trust is explicit; signed identity remains M9. |
| 11. Existing native GUI probe | App/ViewModel unit fixtures establish no local owner, correct command availability and no auto-connect on reattach. The real GUI actor performs fake setup, tracked action/lookup, crash recovery, close and reopen against the same owner in `server_process`. [Native window capture](evidence/m8-gui-probe-connected.png) shows the existing development GUI connected with an empty profile and disabled TV actions. |
| 12. Automatic bounded reconnect | Four fake-clock policy scenarios cover immediate/cap/reset/flapping/sleep/blocked/exit. Real process outage recovery preserves selection with a disconnected TV, never replays setup/connect/action, and requires explicit Resume after Stop. Actual rotation/revocation fixture verifies the same actor against HTTP/bootstrap. Proved quiescing/busy readiness is transient; invalid proof stays blocked. |
| 13. Load/memory/progress | Actual 128 incomplete headers expire; shutdown is finite. Slow mutation/failed-auth and stalled large-response fixtures preserve reserved management/other-client queries. Reservations last through connection completion. Flush deadline covers trickling writes. `control_http_heap` holds eight initial SSE frames and eight query responses before consuming them: 55,838,892-byte combined Rust-heap peak, below 128 MiB. [Measurements](milestone-08-implementation.md#measurements-and-validation) distinguish heap/RSS and reachable limits. |
| 14. Fake profile cannot be promoted | Strict development document envelope rejects renamed/copied records in production adapters; private adapters reject untagged/wrong/duplicate envelopes. Actual process Pair/restart/explicit reconnect/action use separate synthetic presence markers. No production credential or TV adapter is reachable from this composition. |
| 15. Wire/contract/examples/corpus | `control_wire` checks every native/request shape and bounds. Three `control_documentation` tests parse all 12 proposed MCP mutation samples with the real HTTP parser, check full epoch/scoped handles and counter strings, and parse published JSON examples/Stop with native codecs. Fresh-profile semantics, cancel/recovery guards and stable expiry are recorded in [wire](milestone-08-wire.md), [contract](milestone-06-contract.md), [examples](milestone-06-api-examples.md) and corpus. M10 proxy overruns remain explicit. |
| 16. Required checks/review/handoff | `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` and the full `cargo test` pass; [captured output](evidence/m8-validation.txt) records commands and source/fixture fingerprint. Contribution guide records exercised fake-server/GUI-probe/capture commands. The implementation record, this review, bootstrap transcript, architecture and successor handoffs identify concrete interfaces and the remaining signed integration work. |

## HTTP fault coverage and effect assertions

The HTTP fixtures use Hyper over literal loopback, the real application owner
and either actual isolated stores with injected failing adapters or explicit
setup/TV ports. They never synthesize an admitted result in a handler.

| Boundary | Actual fixture and checked facts |
| --- | --- |
| Session/probe/discovery | `every_gateway_and_discovery_failure...` and `pairing_and_connect_session_failures...`: every typed session error in probe, Pairing/connect and authorization; both discovery errors; retained status/code and no unrelated write, packet or credential commit. |
| Setup port | `every_setup_boundary_error...`: all pre-commit SetupError variants return retained HTTP failures; approval may have occurred, but secret/trust/device writes remain zero. PartialCleanup uses the actual journal tests instead of an impossible markerless fake state. |
| Durable Pairing stores | `pairing_storage_faults...`: each credential/trust/device failure retains the exact prepared/token-saved/trust-saved marker. Only explicit guarded recovery removes the reserved ID; failed compensation stays partial. No connection is silently adopted. |
| Reconnect/rotation | `reconnect_material_and_rotated_credential_failures...`: every ReconnectError, missing/changed material and journaled token-save failure; no remote write or automatic new setup. |
| Selection/config/Forget | `failed_selection_wake_edits_and_partial_forget...`: both repository errors, exact per-store removal facts, recovery of remaining steps and no TV I/O. |
| Remote/Wake | `remote_and_wake_terminal_facts...`, `wake_cancellation_over_http...` and complete-setup fixture: written/not-sent/uncertain/disconnect/token rejection; every local send error; cancel before, during and after possible packet send; reachable disconnected Power sends no packet. GET lookup preserves facts. |
| Late work/shutdown | `generation_changes_cancel_late_pairing_and_connect...`, `storage_deadline_and_supervisor_failure...`, `drain_retains_storage_uncertainty...`: late approval/session never commits/adopts; deadline/supervision remain terminal; drain preserves unknown storage and cancellation, including stable terminal facts. |
| Admission | `policy_rejections...` and `owner_effect_and_remote_fifo_overload...`: target/generation/configuration/confirmation/label/Wake/handle errors; malformed ingress creates no operation; rejected effects/FIFO work is retained without dispatch. All 33 accepted FIFO operations write once. |

Source: [HTTP scenarios](../../tests/control_http.rs),
[storage fault ports](../../tests/support/storage_faults.rs),
[wire projections](../../tests/control_response.rs).
Defensive nested variants also have golden status/body projections where an
earlier HTTP/owner guard rejects the request (for example target mismatch or
invalid labels). The real-port cases verify those earlier guards admit no TV
effect; the golden fixtures retain complete enum coverage.

## Session and crash evidence limits

The crash test uses `_exit` in a child at the actual metadata writer's private
checkpoints, skipping destructors and cleanup. It verifies process-crash
behavior and authoritative marker selection, not power-loss certification.
An unacknowledged marker already renamed into place conservatively blocks
start; only completed directory sync permits successful durable acknowledgment.
Existing markers are never erased by a failed replacement.

Distinct profile hashes/socket paths plus an actual shared-port collision test
prevent cross-profile attachment. The injected wrong-UID/code verifier rejects
another principal. SIGTERM models logout, fake-clock suspension models sleep,
and the GUI process fixtures model exit/reopen and server death. These prove M8
state transitions, not real signed multi-user/login-session integration.

## Handoff and remaining phase work

- M9 receives `control_client::{background,http,bootstrap,projection,reconnect}`,
  the early `presentation/iced/remote_probe` wiring, `server::{entry,launch}`,
  lifecycle/endpoint and wire contracts. Extend full GUI workflow parity and
  compose production adapters only after signed peer verification and fresh
  Keychain provisioning pass. Call lifecycle settlement after listener closure.
- M10 receives stable GUI/MCP identities, shared access/admission, private
  operation lookup, lossless codecs, recovery commands and stable `/mcp` URL.
  The HTTP MCP adapter/helper is still unimplemented. The updated 15-tool corpus
  exceeds proxy budgets for the upfront catalog/Pair workflow; real client
  calibration and optimization remain M10, without removing safety facts.
- M11 retains signed upgrade, actual logout/sleep/fast-user-switch, distribution
  and real-TV integration. M6 final signed acceptance is not inferred from M8.

All TV, discovery, Wake and credential effects are synthetic. The development
peer verifies effective UID only and cannot establish signed-code identity.
Default GUI production cutover remains M9; M8's explicit probe has no local TV
runtime and imports no legacy data or credentials.
