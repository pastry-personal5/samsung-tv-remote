# M8 implementation record

Status: Done. The fake-profile HTTP/SSE server, same-executable detached
lifecycle, protected bootstrap and existing-GUI development probe are implemented.
This record supports the [completion review](milestone-08-review.md) and records
the concrete fake/signed boundary.

## Implemented boundary

`main` parses modes before Iced or production adapters. `--server` composes only
`DevelopmentProfile`, a headless `Runtime`, access/bootstrap, lifecycle and HTTP.
The default GUI retains its previous composition. `--gui-probe --fake-profile`
constructs the native HTTP/event client without `InProcess` or local TV services.
The profile starts empty in `m8-fake-v1`; the legacy adapters reject that
namespace, including canonical symlink aliases. Development JSON has a strict namespace
envelope, so renaming directories or copying individual records cannot promote
them into the production format; private adapters also reject untagged documents.
Device/trust/recovery files use
checked owner-only directory handles. Synthetic credentials persist only a
presence marker; loading regenerates the synthetic token. No Samsung, DNS,
Keychain or UDP adapter is reachable from the development composition.

`--server-start` attaches or acquires the profile lock, prebinds the persistent
literal loopback endpoint and passes both descriptors into the same executable.
A bounded report pipe distinguishes child initialization failure. Darwin spawn
creates a new session, clears inherited signal state, uses `/dev/null` stdio,
and inherits only lock/listener/report. No login service is installed. Parent
exit, no connected GUI and client disconnect do not stop the server. Build
identity is SHA-256 of the executable. Normal attach never replaces an
incompatible owner; explicit GUI restart durably Stops it, awaits settlement and
clears only its own exact marker before launching a replacement.

The [bootstrap review](milestone-08-bootstrap.md) records the protected-channel,
one-use HTTP proof and credential release ordering, bounds and fake trust
limitations. Signed peer identity and production Keychain provisioning remain
M9; these development adapters provide no signed-code assurance.

## API, owner and transport

- Strict request DTOs cover every route, confirmation, selected generation,
  config revision and guarded recovery. IDs/counters/expiry are fixed by the
  [wire decisions](milestone-08-wire.md). Handlers query/submit the typed facade;
  they hold no TV or profile-storage adapters.
- Safe exhaustive result/error projections never serialize Debug, tokens or raw
  certificates. The owner captures terminal selection/configuration facts and
  display expiry once. Pending has no expiry. Retention/handle authority uses
  monotonic time. Atomic snapshots include at most 64 private recent references.
- Authentication binds stable GUI/MCP identities and capabilities. Revocation
  takes the write side of the same authorization guard held by synchronous
  owner admission. Queued-but-unadmitted work cannot start afterward. Already
  admitted work retains its outcome. Rotation closes old HTTP/SSE connections;
  a fresh authenticated subscription starts with an atomic snapshot.
- Hyper is the only HTTP framing parser. A bounded raw-header audit retains
  rejection bits for CL+TE and duplicate framing fields normalized by Hyper.
  Headers/bodies are capped at 16 KiB; header count is 64. Headers have five
  seconds, bodies ten, handlers fifteen, ordinary connections thirty. Every
  output batch must flush in ten seconds, including trickling partial writes.
  JSON has a 4 MiB cap. SSE heartbeats occur every fifteen seconds, with bounded
  frames, consecutive revisions and resnapshot/EOF on lag; no replay promise.
- There are 128 connections and 64 ordinary slots, partitioned as 52 mutation,
  eight query and four management. Per identity: ten mutation, four query, two
  management. With the two provisioned identities these lower caps bind first.
  SSE has separate owner limits (64 global/four per identity, backlog 64); only
  eight subscribers are reachable with the current two identities. Bad-auth
  response budget is 64/second; valid query/management uses reserved capacity.
  Reservations remain held through connection completion, so slow response
  readers cannot bypass them after serialization. These bounds do not promise
  availability against arbitrary connection denial.
- Sticky Stop fences owner admission before a blocking synced marker write and
  acknowledgment. The native launcher does not turn a known failure into
  marker-observation success and binds an acknowledgment to its exact epoch/key.
  Failed writes retain their key/result, fence and lock; a new
  explicit key may retry. Lost replies cannot cancel Stop. Signals drain without
  creating sticky Stop. Lock release waits for actual worker settlement and
  HTTP/bootstrap listener closure; the completed actor retains its ownership
  guard until composition joins it. Failed owner joins retain ownership,
  including startup failure paths.

The native client attaches immediately, then retries at 1, 2, 4... seconds,
capped at one hour while open, with one bounded attempt/timer. Thirty healthy
seconds reset the delay. Stop, incompatible build and credential/unsafe metadata
gates block automatic retry until explicit resolution. Exit cancels client
work only. The retry actor retains tracking IDs/keys but no mutation payload;
ambiguous admission recovers through GET lookup, never POST replay. A new epoch
clears old pending GUI projections with an honest unknown outcome message.
The existing GUI gains only a connection status bar and gated Resume/restart.

## Evidence

| Test / artifact | Result established |
| --- | --- |
| `control_wire`, `control_response` | Every request/native encoding, canonical IDs, full-width counter strings, exhaustive typed safe projections, stable expiry/terminal facts; malformed or cross-operation setup handles fail. |
| `control_http` | Real authenticated HTTP setup/re-pair/candidate privacy, selection/connect, Wake configuration/Power/cancel, remote duplicate/lost reply, guarded Pair/Forget recovery, stale epoch and private lookup. Gateway/discovery error matrix retains every mapped failure without unrelated effects. Remote dispositions and all local Wake-send errors preserve their facts through lookup. |
| `control_http` event/access/load fixtures | Two clients share consecutive atomic events; rotation closes streams; raw framing/authority/body/header failures never admit; mutation and failed-auth saturation preserve reserved queries/management; eight max-profile streams preserve queries; 128 incomplete headers expire and shutdown is finite. |
| Owner/runtime unit tests and M7 suites | Queued authorization/revocation ordering, overflow discard/resync, admission/retention/uncertainty, stage deadlines, every storage compensation boundary and late-worker settlement. |
| `server_bootstrap` (nine scenarios) | Protected listener proof before reusable secret; real malicious TCP occupant; every context binding; wrong peer/injected identity, blocked credentials, replay, one-use grants, wrong proof, frame/expiry/nonce-capacity limits; rotation during a challenge, quiescing/busy readiness and restricted credential release. |
| `server_lifecycle` | Owner/modes/link/path checks, synced atomic reopen, concurrent first lock creation/read replacement, durable Stop/fence, marker failures/lost acknowledgment, signal/late blocking settlement. |
| `development_profile` | Empty/no legacy import or effects, separate synthetic persistence, restart/explicit action, production adapter and unsafe-path rejection. |
| `server_process` | Actual detached parent-exit survival, simultaneous start/same epoch, stable endpoint, fake Pair/restart/explicit connect/action, sticky Stop/Resume, authenticated incompatibility/explicit restart, occupied/corrupt endpoint rejection, GUI actor outage/new epoch/no replay/exit survival; detached session/descriptor fixture. Stop/start races and separate profile bindings also pass. |
| `control_client_rotation` | Actual GUI actor performs fake setup/action, rotates credentials, takes a fresh same-owner snapshot and blocks on revocation. Exactly one probe/approval/connect/write and no packet; no automatic retries after denial. |
| `control_documentation` | All 12 proposed mutation samples parse with real wire/native codecs; published JSON, full epoch/scoped handles, counter strings and fresh empty profile are checked. |
| `control_client` fake clock | Immediate attach, cap repetition, healthy/flapping reset, sleep/no catch-up overlap, matching blocked resolution and terminal exit. |
| GUI App/ViewModel unit fixtures | Probe has no local owner; reattach preserves identity and never auto-connects or enables TV actions while server is unavailable. |
| [Native renderer capture](evidence/m8-gui-probe-connected.png) | Existing development GUI visibly connected to the isolated server; empty first profile and disabled TV controls. Captured from this window only, no desktop content. |

The concurrent-launch regression reproduced a transient `ENOENT` while first
creating `owner.lock`. The checked `openat` path now retries that exact name a
bounded eight times without relaxing mode or no-follow checks. Atomic reads
also re-resolve an inode unlinked during replacement. A 16-round/eight-thread
fixture requires exactly one owner, and concurrent atomic reads accept only
complete old/new contents. Subprocess concurrent starts then passed together
with the other lifecycle scenarios. Temporary diagnostic logging was removed.

A second race exposed the Unix socket before its final permissions were set.
It now binds privately, validates mode, and publishes exclusively with Darwin
`RENAME_EXCL`. A deterministic fixture injects a competing socket at publication;
it is preserved. Proved quiescing/busy readiness and invalidated rotation grants
are transient, while invalid proof and actual revocation remain blocked.

The actual writer's private checkpoints are exercised in abrupt child exits at
create/write/file-sync/rename/directory-sync, with absent and existing markers.
Only complete old/new authoritative files survive; temporary entries do not
become Stop authority. This models process crash, not power failure.
Drain now captures stable terminal facts through the same owner finish path as
all other outcomes and emits ordered coarse completion events.

The maximum profile exposed the former 1 MiB disk cap as smaller than valid
1,024-record JSON. Files now cap at 4 MiB, preserving the explicit record/field
limits and rejecting oversize instead of truncating. The actual persistent
profile is exercised in HTTP and isolated allocator fixtures.

## Measurements and validation

Measured on 2026-11-12, debug stable Rust/macOS, synthetic data only:

| Workload | Measurement |
| --- | ---: |
| Pure max-profile DTO, 1,024 records, maximally escaped labels/hosts/full-width counters | 2,051,573 JSON bytes against 4,194,304 cap |
| Actual max-profile persistent file | 1,953,781 bytes |
| Actual owner HTTP state | 2,032,121 JSON bytes |
| Actual initial SSE frame | 2,032,151 bytes |
| Isolated full-profile HTTP + eight SSE frames + eight query responses held before reading | 55,838,892-byte peak Rust heap increase; 578 ms |
| Concurrent HTTP suite's eight snapshots and queries | 1,247 ms |

`control_http_heap` is a separate test binary with a counted System allocator.
Its conservative 128 MiB regression ceiling includes fake profile, runtime,
server serialization and test-client allocations on both sides, with overlapping
unread initial frames/responses. It excludes
kernel socket memory, GPU allocations and macOS RSS; it is not a server-only
RSS or latency guarantee. Ordinary tests do not allocate the measurement
allocator. M7's registry/effect/backlog measurement remains separate.

Final validation on 2026-11-12 is recorded in
[evidence/m8-validation.txt](evidence/m8-validation.txt): formatting, Clippy with
`-D warnings`, the complete Cargo suite, the isolated heap workload and the
corpus measurement all passed. Current coverage includes 109 library tests, 26 actual HTTP
scenarios, nine bootstrap scenarios, eight lifecycle scenarios, eleven actual
process scenarios, three development-profile and three documentation scenarios,
the native GUI rotation fixture and the isolated heap fixture, plus the full
M7/regression suites. No denied loopback binding is treated as a passing test.
The corpus proxy overruns remain M10 work, not passing model-token budgets.

## Review and handoff

The [completion review](milestone-08-review.md) maps all sixteen overview items
and the architecture verification matrix to actual source/tests/artifacts. It
separates real loopback/process/native-window evidence, injected TV/credential
ports, deterministic session models and the signed M9/M11 work. The contribution
guide records exercised commands; contract/examples and the proposed MCP corpus
share the tested M8 representations. No production GUI cutover, real TV or
production pairing credential was used for M8.
