# P3-M1 implementation — capability foundation

Status: Active — foundation and diagnostic slice implemented; hardware and release acceptance open.

## Authorization and delivered scope

The owner's 2026-11-14 implementation request authorizes the offline code and
fake-session checks recorded in the [overview](milestone-01-overview.md#implementation-authorization--2026-11-14).
This foundation proceeds ahead of the diagnostic spike so the semantic owner
policy and read boundary can be reviewed without inventing Samsung frames.
It does not resolve Phase 2 acceptance or release the new MCP result shape.

- A finite domain vocabulary separates discovery, launch, native IME and five
  individual source keys. Unknown facts contain no evidence or timestamps.
  Sources require user evidence; an IME lifecycle event cannot prove launch.
- The application retains one observation per saved TV/key in memory, with
  evidence source, wall-clock display time and epoch/selection/session scope.
  A separately supplied monotonic clock governs optional expiry. Duplicate
  observations neither extend deadlines nor publish repeated revisions.
- The runtime synchronizes invalidation before lifecycle publication. Old
  sessions cannot revive support or live IME state. History remains stale;
  Forget removes it, and restart reconstructs no history. There is no new
  disk format or durable catalog.
- Content-free semantic IME start/end events provide the fake-session seam.
  A start sets live activity; an unmatched end can establish lifecycle support
  without opening a field. No production Samsung parser emits these events yet.
- HTTP state/detail, SSE, typed native snapshots and MCP `inspect_tv` share
  per-TV summaries. Recovery-only targets have no invented TV facts. No new
  MCP tool, refresh, request trigger, text control or source key is introduced.
  `list_tvs` retains its compact projection.
- Compatible older responses with no capability field decode as unknown.
  Malformed facts, duplicate/reordered keys, lossy counters, wrong targets,
  and contradictory current epoch/selection/readiness fail closed.

The summary's `reason` describes its connection/selection context. Individual
facts still carry unknown/current/stale evidence independently; the typed
`reason_for` policy combines them. `connected` in the context is not a universal
support grant or mutation admission. All existing mutations retain their guards.
Firmware clues and negative wire-response attribution await reviewed evidence;
no model/research match or successful write creates a fact.

## Deterministic evidence and measured bounds

`tests/capabilities.rs` checks compatible decoding, malformed/current evidence,
lossless counters and the maximal profile. Application policy tests use an
explicit clock and paused Tokio time for expiry, including backward wall-time
observations. They cover per-TV/key isolation, invalid sources, negative evidence,
all identity fences, duplicates, unmatched ends, Forget and restart.

The shared fake HTTP/MCP listener tests compare native, SSE, detail and
inspection at one revision. Same-TV selection, disconnect and token rejection
invalidate live state without a reconnect or replay. Reads and reports send
zero remote actions. The fixture supplies synthetic semantic events only;
these tests establish no hardware compatibility.

The adversarial 1,024-TV profile includes eight supported historical facts per
TV, maximum counter widths, maximum escaped labels/hosts and both configured
Wake interfaces. Its snapshot is **4,071,889 bytes**, below the existing
**4,194,304-byte** HTTP/native limit with room for event framing. The first
representation exceeded the cap; sharing `evidence_epoch` once per TV fixes
that without raising a transport limit or truncating evidence.

A synthetic paired, unobserved inspection measured actual server JSON:

| Sample | UTF-8 bytes |
| --- | ---: |
| Complete `tools/list` result, including all output schemas | 58,809 |
| `inspect_tv` result, including structured and text channels | 3,750 |

The catalog still has 15 definitions. These are server byte measurements,
not installed Claude Code token counts or budget acceptance. The Phase 2
corpus remains a historical baseline; the new fake inspection sample lives
in `capability_inspection_budget_sample_is_bounded_and_keeps_fifteen_tools`.
Actual installed-client catalog/result/workflow measurements and any required
cuts or explicit re-baseline remain a release gate.

## Diagnostic slice — 2026-11-14

The `p3-spike` Cargo feature implements the [private contract](milestone-01-diagnostic-contract.md).
Default builds expose neither private routes nor controller. One protected,
stream-owned lease captures the ready TV/session; controller loss, authority
revocation, expiry, pressure and identity fences close it. Normal mutations
return retained busy rejections while reads remain available. A canceled
unsent request has a revoked permit; admitted outcomes remain written or
uncertain and are never replayed. Duplicate UUID intents are guarded by finite
experiment and current alias revision.

The Samsung diagnostic actor has bounded candidate encoders/parsers for catalog,
ordinary native launch, individual source keys and passive IME lifecycle. It
marks uncorrelated matching events ambiguous and quarantines a timed-out family
until explicit reconnect. App identities stay in memory; exported fixtures use
synthetic replacements. Candidate observations never update product capabilities.
No generic frame sender, parallel socket or text operation was added.

The foreground `--p3-spike` controller attaches through existing signed bootstrap,
never starts/resumes/connects/pairs, requires one typed confirmation per effect,
and writes review-required drafts. The candidate builder records commit/dirty
metadata before Apple signing at a new preserved path. Static candidate
inspection verified the signature without Keychain calls. The installed owner
was inspected read-only and was serving; the temporary owner switch was held
for explicit operator approval, separately from TV-effect intent.

Focused policy, codec, export, HTTP lease, fake foreground-controller and bundle
signing tests passed. The fake HTTP test found and fixed a real publication gap:
status-triggered lease cleanup now publishes NotConnected atomically. All
candidate observations and fake exports remain diagnostic/synthetic rather than
claimed production support or target-TV evidence.

## Remaining milestone work

1. Obtain the explicit temporary owner-switch approval and run the hands-on
   matrix with individual effect intent, fresh aliases and separate panel
   confirmation. No TV request or target capture is claimed by preparation.
2. Review captured/redacted event shapes, correlation, timeout quarantine,
   parser bounds and queue behavior before implementing production acquisition.
   The production Samsung authorization parser is unchanged; production
   capability facts stay unknown. Candidate source/app/IME wire schemas are
   not frozen by the semantic JSON projection.
3. Implement any evidence-backed firmware metadata, lifecycle parsers and
   request attribution tests. Complete signed-native and installed-client budget
   gates, update the checklist and record downstream scope decisions.

P3-M1 is **not Done**. Passing the offline suite cannot replace these gates.

## Validation

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --all-targets -- -D warnings` — passed.
- `cargo test --quiet` — full suite passed against a stable executable,
  including all 20 process tests.
- Focused capability policy, native projection, shared HTTP/MCP and measured
  budget tests — passed.
- `git diff --check` and local documentation file-target checks — passed.

An initial run found the restart test's obsolete whole-device JSON comparison;
it now checks preserved configuration and separately verifies unknown capability
state after restart. A helper cold-start timeout passed on isolated rerun.
A subsequent run was invalidated by rebuilding the executable concurrently
with process identity checks. The final complete run held the executable stable
and passed. Only the two disposable fake owners left by that interrupted run
were terminated, after exact identity/start-time and removed-profile checks.

The operator subsequently confirmed that a hands-on TV session is available.
That is availability, not per-effect source/launch intent or protocol evidence;
the diagnostic controller still has to be prepared before the live spike.


## Final diagnostic preparation validation

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --all-targets -- -D warnings` — passed after fixing the
  disabled-feature branch's infallible destructuring lint.
- `cargo test --quiet` — full default suite passed.
- `cargo clippy --all-targets --features p3-spike -- -D warnings` — passed.
- `cargo test --features p3-spike --quiet` — full feature suite passed,
  including 21 process tests, the fake foreground controller, private HTTP
  lease, alias-revision conflicts, cancellation/pressure and export checks.
- `make bundle-p3-spike-candidate` — built a preserved Apple-signed candidate;
  its static `--server-api-access --inspect` passed with no Keychain request.
- `make server-status` — the installed signed owner was serving; this read
  started no owner and sent no TV command.
- Offline recorder prepared a scratch 30-row matrix with every row untested,
  using the signed executable's bundled commit/dirty provenance and UTC date.

The temporary owner switch is awaiting operator approval because it disconnects
all clients. Per-source and per-app intents remain separate. No real TV frame,
visible source/app result, IME event, protocol fixture, production schema freeze
or installed-client token-budget acceptance is claimed by these checks.
