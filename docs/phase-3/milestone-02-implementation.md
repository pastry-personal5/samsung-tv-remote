# P3-M2 implementation — Sources

Status: Active — offline implementation complete; hardware and release
acceptance open.

## Authorization and decisions

The owner's 2026-11-15 request authorizes offline code and fake tests ahead of
P3-M1 and Phase 2 acceptance; see the
[overview](milestone-02-overview.md#implementation-authorization--2026-11-15).
Two gate decisions were implemented at the recommended default; the owner
accepted both on 2026-11-15:

- **Tuner:** configurable like an HDMI port, not always shown.
- **Editing:** GUI (through the authenticated HTTP route) only. MCP reads
  `sources` in `inspect_tv` and sends the five new `remote_action` names. No
  MCP tool was added; the catalog stays at 15.

## Delivered

- **Domain.** `RemoteAction::{Hdmi1, Hdmi2, Hdmi3, Tuner, SourceMenu}` join
  `ENCODED_ACTIONS`, not `LIVE_ACTIONS`. `source_capability` is the one
  action-to-fact map (`SourceMenu` → `sources`). The codec maps them to
  `KEY_HDMI1`–`KEY_HDMI3`, `KEY_TV` and `KEY_SOURCE`. The frozen diagnostic
  copy is unchanged; a `p3-spike` test asserts identical frames.
- **Model.** `application/sources.rs`: per-port Unconfigured / Hidden /
  Shown(label, confirmed time). Labels are trimmed, 1–24 scalars and ≤64
  bytes, and reject controls and bidi controls rather than stripping them.
  Showing sets the confirmation time; relabelling keeps it; hide/reset clears it.
- **Facts.** Shown ports project as durable `supported`/`user_confirmation`
  facts with no context, overlaid after the session history, so a session
  observation cannot revive a hidden port. Future times are stale. Server
  validity and every native decoder fence (current readiness, blocked
  startup/admission, common session) exempt only these facts.
- **Mutation.** `Command::SetSources` is a full four-entry replacement sharing
  the Wake config revision. It joins both selection-fence arms
  (`Command::metadata`, `execute`). `StorageWrite::{Select, Wake, Sources}`
  replaces the `Option<WakeConfiguration>` overload; only Select fences the
  session. Lost-acknowledgment reconciliation compares only the write's
  target and kind. The operation registry gives identical retries their
  receipt and conflicts a changed payload.
- **Admission.** After the selection fence and before the dispatcher, an
  Unconfigured or Hidden port is rejected `source_unconfigured` /
  `source_hidden` (HTTP 412) and never sent. Configuration is read once at
  admission; a later edit does not recall a queued key.
- **Storage.** `devices.json` v3 with a typed, strict `sources` block;
  `devices.pre-v3.json` backup on the first write through save, select or
  delete; pairing backups v2. See [architecture](../architecture.md#saved-tv-storage-format).
- **Projections.** `PUT /v1/devices/{id}/sources` with a typed body that
  rejects missing, duplicate and unknown keys, labels on non-Shown entries,
  invalid labels and client timestamps. The owner re-baselined the MCP catalog
  at 58,820 bytes (+83) on 2026-11-15. The `sources` block appears in device
  detail, `inspect_tv` and the selected snapshot entry only. The `inspect_tv`
  output schema declares `sources` as a plain object: a full per-entry schema
  cost about 1.3 KB of catalog, which the budget has no room for.
- **GUI.** The Sources view lists Shown keys as "Request <label> — <port>",
  always offers the TV's Sources menu, and reuses the Remote view's disabled
  explanation. Results read "source request sent" or "may have been sent";
  nothing is replayed. Settings → Sources offers per-port Show (with a second
  confirmation), Hide, Reset and a label. Each change is one explicit save
  at the observed revision; edits are refused while a save is unresolved,
  and drafts are discarded on selection change.

## Evidence

Deterministic tests, all with fakes: domain map; label rules; confirmation
time; durable fact validity and overlay; storage migration from v1 and v2
through each write path, backup reuse, conflict and unwritable destination;
strict v3 decoding; pairing backup v2/v1; admission precedence and effect
counts; shared revision both ways; no session fence on save; uncertain-save
reconciliation; HTTP round trip and malformed bodies; native decode and
old-response fallback; GUI editor submission; 1,024-TV snapshot, SSE and disk
size profiles. Measurements are in the [changelog](changelog.md).

## Open

- Hardware matrix ([overview](milestone-02-overview.md#hardware-matrix)): no
  key has been sent to a real TV.
- A deterministic hidden-after-queue test (a queued key still sends after a
  hide) is not written; the behavior follows from admission-time reads.
- Fault injection between the backup and live renames relies on the existing
  atomic private-file primitive; only destination-level failures are tested.
- Native keyboard/VoiceOver review of the new views.
