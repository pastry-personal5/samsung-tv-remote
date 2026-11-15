# P3-M1: Capability Model and Protocol Evidence

Status: Active — capability foundation and gated diagnostic controller implemented; target-TV evidence and acceptance gates open.

## Goal and entry gates

Establish what the paired local connection can demonstrate on the target TV,
then expose server-owned capability facts to HTTP, events, and MCP. Keep those
facts separate from connection readiness, permission to act, and request
outcomes. A successful write never proves a source, app, or text-field state.

The [phase plan](phase-3.md) owns scope and ordering. The
[architecture](milestone-01-architecture.md) defines the proposed model,
ownership, invalidation, and implementation seams. The
[plan review](milestone-01-plan-review.md) records the corrected gaps and
validation for this revision.

Phase 2 is still Active. Planning P3-M1 does not authorize implementation ahead
of its entry condition, waive native acceptance, or resolve the pending MCP
budget decision in [Phase 2](../phase-2/phase-2.md#owner-decisions). Start
implementation after Phase 2 is Done, or after a separately recorded owner
exception identifying which work may proceed. Diagnostic preparation may
proceed under this review request; the owner’s implementation request now covers the gated diagnostic code;
physical effects still require per-experiment intent. Do not freeze
production Samsung schemas until the target-TV spike has evidence. No spike has been run for this plan.

## Implementation authorization — 2026-11-14

The owner requested “Impl. P3-M1.” This authorizes offline code and fake-session
validation ahead of Phase 2 completion. It does not accept Phase 2, establish
installed-client budget headroom, authorize physical experiments without their
per-effect intent, or supply target-TV captures. The bounded semantic model
and read projections can be reviewed independently of Samsung wire shapes.
The [implementation record](milestone-01-implementation.md) identifies this
out-of-order foundation and the remaining diagnostic/evidence work. P3-M1
remains Active; no completion checklist item is satisfied by an invented capture.

## Scope

- A target-TV evidence record and sanitized fixtures for installed-app replies,
  launch replies or silence, IME start/end, and direct-source-key effects.
- A small domain vocabulary and application-owned per-saved-TV capability
  projection, including evidence source, observation time, freshness, and an
  optional firmware clue only when actually supplied by the TV.
- Session fencing and invalidation, with deterministic clocks and fake ports.
- Read access in existing authenticated HTTP snapshots/detail, SSE state, and
  MCP `inspect_tv`; typed native-client decoding of the same facts.

Source configuration and labels belong to M2; persistent installed-app catalogs
and launch controls to M3; text send, drafts, and text-session mutations to M4.
M1 may recognize a current IME event but adds no text entry control. It adds no
generic frame sender, new MCP tool, cloud transport, or TV-state monitor. The
GUI's feature placeholders remain until their respective milestones.

## Delivery order

| Slice | Work | Exit evidence |
| --- | --- | --- |
| 1. Predecessor review | Record Phase 2 completion or the precise owner exception, current build identity, and accepted MCP budgets. | Linked gate record; no assumed acceptance. |
| 2a. Diagnostic preparation | Implement and fake-test the gated owner harness in the [automation contract](milestone-01-spike-automation.md); candidate schemas are diagnostic hypotheses only. | Exclusive session lease, per-effect intent, bounded observations, redaction and no-replay proofs. |
| 2b. Target-TV spike | Use that paired server-owned session to collect the matrix below. Inspect and sanitize each retained frame before committing it. | Evidence record distinguishes observed, inconclusive, unsupported, and untested results; fixture provenance reviewed. |
| 3. Model freeze | Reconcile evidence with research; settle named event schemas, negative evidence, bounds, and policy. Record downstream scope decisions. | Architecture and phase changelog updated before production parsing depends on a shape. |
| 4. Application and adapter | Extend typed session events and the runtime's per-TV facts; fence late events and invalidate session facts. | Fake-clock, codec, and lifecycle tests, including exact effect counts. |
| 5. Client projections | Add bounded HTTP/SSE and typed-client projection; extend `inspect_tv` and its result corpus. | HTTP/MCP agreement, atomic subscription and budget evidence. |
| 6. Acceptance | Run required checks; review evidence, current architecture and downstream handoffs. | Every completion item below has evidence or remains explicitly open. |

## Target-TV spike

The [automation workflow](milestone-01-spike-automation.md) defines executable
offline recording, the missing live-harness prerequisite and the division
between machine observations and human panel confirmation. The private
feature-gated controller is implemented; preparation does not claim hardware
execution. The ordinary public API cannot perform this spike.

Schedule a hands-on session for the saved target TV, currently documented as
`KU75UA8090FXKR`. Normal paired TLS trust, token handling, selection guards and
one-session ownership still apply. Prepare a narrowly scoped server-side
diagnostic harness using existing gateway/session seams; the GUI, MCP adapter,
and external scripts must not open a parallel TV-control path. The harness
accepts only the finite experiments below and is excluded from routine startup
and public API/tool catalogs. It must not log raw inbound frames or private
payloads. Use synthetic app names/IDs in committed fixtures while preserving
the observed structure, types and relevant string-encoding behavior.

Source changes, launches and any test typing need explicit intent at the
hardware session. This planning request does not initiate them. Navigate to a
non-sensitive native field manually; no password, account form, clipboard,
blind menu traversal or automatic form submission is part of the spike.

| Experiment | Evidence to record | Interpretation |
| --- | --- | --- |
| Baseline | Build identifier, model, paired connection, test date; firmware only if actually reported, with provenance. | Model/research alone grants no capability. |
| Installed apps | One explicit request, actual reply envelope and types, count/size, empty-list behavior if observable, and deadline. Repeat after reconnect and manual Wake/reconnect; reacquire IDs for each new session. | Valid reply proves discovery for that TV/session, not launch or foreground state. |
| Launch | Two IDs returned by that TV; ordinary home launch without content parameters; reply/error/silence and separately noted human-visible effect. Repeat after reconnect and manual Wake/reconnect using freshly returned IDs. | Silence and a socket write are inconclusive about success. A human observation is user evidence. |
| Native IME | Manually open and close a harmless native field; record start/end event names and shapes, ordering, duplicate events and reconnect behavior. | No event within a bounded window is inconclusive, not proof of universal lack of support. |
| Sources | Explicitly test each proposed HDMI key, tuner where configured, and Sources menu; repeat after Wake/reconnect. Record human-visible effect or why untested. | No automatic proof exists; confirmation applies only to the tested key and TV. |

Create `docs/phase-3/milestone-01-evidence.md` during the spike. Include the
matrix, observation windows, manual steps, sanitized fixture paths, redactions,
and unresolved questions. Put reusable protocol fixtures under
`tests/fixtures/samsung/phase-3/` with a provenance manifest. Do not create
purported captures in advance. Unknown-frame and malformed-input fixtures may
be synthetic but must be labelled as such.

Never commit addresses, MACs, pairing tokens, channel/client identifiers,
typed text (including Base64), account data, or full unsanitized diagnostics.
IME fixtures retain only the event structure needed to establish lifecycle;
remove field contents and identifiers. Keep any firmware clue distinct from
a firmware version read manually from Settings.

## Completion checklist

- [ ] Entry gate is resolved with links. Phase 2 waivers retain their original
  scope; they are not silently extended to Phase 3 hardware evidence.
- [x] Live owner harness is explicitly gated, finite and fake-tested. Controller
  loss releases its lease, late replies after timeout cannot satisfy another
  request, and no external script opens a parallel TV-control connection. See
  the [diagnostic preparation validation](milestone-01-implementation.md#final-diagnostic-preparation-validation).
- [ ] Target-TV matrix and reviewed fixtures identify what was actually
  observed. Silence, timeout and malformed replies do not become `unsupported`.
  Untestable rows remain open or receive a recorded scope decision.
- [ ] Every fact is scoped to a saved TV and has explicit support, source,
  time and freshness. Unknown facts have no invented timestamp or firmware.
  Catalog discovery, launch, each source key and native IME remain distinct.
- [ ] Disconnect, session replacement, selection change, re-pair, Forget,
  token rejection, certificate mismatch and restart invalidate the appropriate
  facts. Late frames from an old session cannot revive them. Fake-clock tests
  cover expiry and clock changes without sleeps.
- [ ] Parsers accept only evidence-backed, named bounded events. Tests cover
  malformed/oversized/unknown/duplicate/late frames, string sanitization and
  backpressure. Sensitive IME data never reaches application events or logs.
- [ ] HTTP snapshots/detail, SSE, typed native-client projections and MCP
  `inspect_tv` agree about the same revision and TV. Reads send no TV request;
  two clients neither create duplicate probes nor retarget evidence.
- [ ] Capability reads do not hide tools or imply action eligibility. The
  MCP catalog stays within the accepted Phase 2 baseline and result/workflow
  budgets are re-measured; any overrun needs cuts or an owner re-baseline.
- [ ] M2–M4 handoffs record supported paths, inconclusive evidence, size/expiry
  decisions still needed and any narrowed scope. An absent IME event does not
  silently authorize the M4 focused-field fallback.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo test` pass for the implementation. Affected current docs and the
  phase changelog are updated; no planning item is reported as executed proof.

## Downstream handoff

M2 owns per-key confirmation/configuration and its durable revision guards.
M3 owns catalog expiry, refresh/coalescing, bounded persistence, migration and
Forget recovery. M4 owns send limits, secret-safe operation deduplication, IME
session handles and the MCP-text decision. The spike informs these decisions;
it does not ship their controls. If a feature is demonstrably unsupported,
record the evidence and narrow or drop its milestone in the phase plan. If
evidence is only inconclusive, retain that distinction and the open gate.
