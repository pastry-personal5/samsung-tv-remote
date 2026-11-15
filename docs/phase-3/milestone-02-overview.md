# P3-M2: Sources

Status: Active — offline implementation 2026-11-15; hardware, MCP budget and release acceptance open.

## Goal

Replace the Sources placeholder with user-configured direct-input keys and the
Sources menu fallback. Each key goes through the existing finite remote-action
path and is reported as a request ("Source request sent"), never as an active
input. The [architecture](milestone-02-architecture.md) defines the model,
storage, admission and projections. The [plan
review](milestone-02-plan-review.md) records corrections against the current
code and remaining owner decisions.

## Implementation authorization — 2026-11-15

The owner requested “Impl. P3-M2.” Like
[M1's](milestone-01-overview.md#implementation-authorization--2026-11-14), this
authorizes offline code and fake-session validation ahead of P3-M1 and Phase 2
acceptance. It does not supply hardware evidence or complete M2. The owner
then accepted the implemented defaults and the budget on 2026-11-15: the
tuner is configurable like an HDMI port; source editing is GUI/HTTP only (MCP
reads through `inspect_tv` and sends through `remote_action`); and the MCP
catalog is re-baselined at 58,820 bytes (+83). The [implementation
record](milestone-02-implementation.md) lists what was built and what is open.

## Entry gate

- P3-M1 is Active. Its offline capability foundation exists; its spike has not
  run. Phase 2 is not Done.
- M2 parses no new inbound frames. Its offline work therefore needs no captured
  Samsung shapes, only an owner implementation exception like
  [M1's](milestone-01-overview.md#implementation-authorization--2026-11-14).
- The per-key hardware results, including after Wake and reconnect, need a
  hands-on session. That gate blocks acceptance, not offline code.
- The MCP additions needed a measured offset or an owner re-baseline (see
  [phase rules](phase-3.md#phase-wide-rules)); the owner re-baselined the
  catalog on 2026-11-15.

## Scope

- Per saved TV, the user shows or hides each of HDMI 1–3 and the TV tuner, and
  may label each shown key (for example "PlayStation"). An unlabelled key shows
  its port name. The app never invents device or cable names.
- The Sources menu key is always offered while connected. It opens the TV's own
  menu for manual D-pad use, with no scripted traversal.
- Source keys are new `RemoteAction` values. They take the Phase 1 admission,
  queue and outcome path. Admission rejects a key the user has not shown
  (`source_unconfigured` or `source_hidden`, HTTP 412, like
  `wake_not_configured`); it is never sent. An `uncertain` send is never
  replayed.
- Source configuration is stored in the saved-TV record. It shares the Wake
  config revision, survives restart and re-pair, and Forget removes it.
- The GUI Sources view and the Settings editor. HTTP read and write. MCP gets
  read access through `inspect_tv` and sends through `remote_action`.

## Non-goals

- Any probe, active-source report, or "HDMI 1 active" state.
- HDMI 4 or more ports, CEC device names, or automatic port detection. The M1
  eight-fact contract fixes the key set.
- A new MCP tool. A Wake-plus-source macro or an automatic retry.

## Delivery slices

| Slice | Work | Exit evidence |
| --- | --- | --- |
| 1. Gate | Record the owner exception, the MCP lane choice, the label-editing decision and the tuner reading. | Changelog entry with links. |
| 2. Model and storage | Domain source keys, per-TV `SourceConfiguration`, device format v3 with backup, pairing-backup field and versioned backup envelope. | Migration tests from v1/v2 fixtures; Forget and re-pair-recovery tests. |
| 3. Admission | `SetSources` command, typed storage write, source-key admission and codec mapping. | Stale-revision/selection/key tests; hidden and unconfigured rejection codes and precedence; no replay; exact effect counts with fakes. |
| 4. Projections | HTTP route, snapshot/detail/SSE and native decode, MCP enum and `inspect_tv`. | Round trip, old-response fallback, 1,024-TV size re-measurement, catalog/result bytes. |
| 5. GUI | Sources view, Settings editor, uncertain-save handling. | View-model tests; keyboard, VoiceOver and disabled-reason review. |
| 6. Hardware | Matrix below, through the product path. | `milestone-02-evidence.md` with no addresses or tokens. |

## Hardware matrix

The owner explicitly intends each key at the session. Use the normal GUI or MCP
path; no diagnostic controller is needed. P3-M1's Sources row may be cited for
the same TV, key and connection condition if it already ran; M2 still verifies
admission and outcomes through its product path. Record the tested build and
synthetic TV alias, never network details.

| Key | Cold connect | After manual Wake + reconnect | Record |
| --- | --- | --- | --- |
| HDMI 1–3 | Send once; note panel effect. | Repeat. | Effect, or why untested (for example, nothing connected). |
| Tuner | Send once. | Repeat. | Effect, or no antenna. |
| Sources menu | Send once; dismiss with Back. | Repeat. | Menu opened or not. |

Record the rejection code or the terminal outcome (`not_sent`, `written` or
`uncertain`) exactly as the product reports it;
record panel effect separately. An inconclusive case remains open or needs an
explicit scope decision; a missing antenna/cable is not a passing key test. For
a completed local write, the reported outcome stays `written`. The panel effect
is human evidence only for that key and TV.

## Completion checklist

- [x] Gate recorded: implementation exception, MCP budget lane, label-editing
  decision, tuner reading (configurable or always shown).
- [ ] Only shown keys are admitted. Unconfigured and hidden keys are rejected
  with distinct 412 codes and are never sent; the rejection outranks
  connection and queue guards, after stale selection. The Sources menu needs no
  configuration. Wire names match `RemoteAction`'s PascalCase serde spelling.
- [ ] No result, event or GUI string claims an active input. Writes never set
  support.
- [ ] `SetSources` rejects a stale epoch, selection, generation, config
  revision or malformed replacement (including a stale-generation request
  that a missing wildcard arm would let through), never fences the session,
  and its uncertain save reconciles
  authoritative sources and timestamps before releasing the shared storage
  lane. An identical operation retry returns its retained receipt;
  changed-payload key reuse conflicts without another write.
- [ ] Full replacement requires all four keys exactly once; reset, hidden and
  shown are distinct. Rejections and receipts round trip through both clients.
- [ ] Durable facts decode when disconnected/non-selected/admission-closed;
  session facts retain all M1 fences. Confirmation timestamps persist and label
  or Wake edits do not manufacture fresh evidence. Future timestamps are stale.
- [ ] Labels are trimmed and bounded at the server; unsafe content is rejected,
  not stripped or truncated.
- [ ] Device format v3 migrates v1/v2 records, keeps a documented backup, and
  older builds fail closed, including source-bearing pairing backups. Every
  first-write path and backup failure is tested; downgrade and
  historical-backup retention are documented. Forget, Forget recovery and
  re-pair recovery are tested.
- [ ] Restart keeps configuration. Selection change, disconnect and Forget
  behave as M1 specifies for session facts.
- [ ] HTTP, SSE, native and `inspect_tv` agree at one revision. The 1,024-TV
  snapshot and framed SSE stay under the 4,194,304-byte limit with worst-case
  escaped labels, mixed durable/session facts and maximum counters. Disk bounds
  are measured separately. Omitted unselected details cannot overwrite
  settings.
- [x] MCP catalog stays at 15 tools; catalog growth (+83 bytes) is measured and
  re-baselined by the owner.
- [ ] GUI: keyboard access, VoiceOver labels, disabled explanations.
- [ ] Two-client source/Wake edits conflict at one shared revision. Save
  acknowledgment loss, timeout, restart, selection change, queue pressure and
  hidden-after-queue behavior have deterministic fake tests with effect counts.
- [ ] Hardware matrix recorded; any deferred cell leaves hardware acceptance
  open unless an owner records the resulting scope reduction.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D
  warnings`, and `cargo test` pass; affected docs and the changelog are
  updated.
