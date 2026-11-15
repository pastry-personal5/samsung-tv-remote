# P3-M2 Architecture: Sources

Status: Proposal — planned 2026-11-14; names below describe the contract, not
frozen Rust or JSON spelling.

## Model

Add five `RemoteAction` values: `Hdmi1`, `Hdmi2`, `Hdmi3`, `Tuner`, and
`SourceMenu`. They join `ENCODED_ACTIONS` but not `LIVE_ACTIONS`, which drives
the Remote view and `control_state` statuses. The Sources view owns their
presentation. `samsung/codec.rs` holds the only production mapping
(`KEY_HDMI1`–`KEY_HDMI3`, `KEY_TV`, `KEY_SOURCE`). The diagnostic controller
reuses that mapping rather than keeping its own copy.

`SourceConfiguration` belongs to `SavedDevice`, next to `wake`. For each of
HDMI 1–3 and Tuner it holds one of three states:

| State | Meaning | Admission | Capability fact |
| --- | --- | --- | --- |
| Unconfigured | Default for a new or migrated TV. | `not_sent` | `unknown` |
| Hidden | User preference: do not offer. | `not_sent` | `unknown` |
| Shown(label?) | User chose to offer this key. | Admitted | `supported`, source `user_confirmation` |

"Hidden" is a preference, not negative evidence. It never becomes
`unsupported`. The Sources menu has no state: it is always admitted while
connected, like Home or Back, and its fact stays `unknown`.

**Tuner rule.** The phase plan says to offer the tuner "as an ordinary key". M2
reads that as the ordinary remote path, not as always shown. Many setups have
no antenna, so the tuner is configured like an HDMI port. Record this reading
in the phase changelog.

**Durable facts.** M1's `CapabilityFact::valid` requires a session context for
every supported fact. Source-key facts come from durable user configuration
instead. They are projected at read time with `context: null`, freshness
`current`, and the per-key confirmation time as `observed_at`. Persist that
timestamp with each Shown entry: showing a key establishes confirmation;
relabelling it preserves the time; hiding or resetting it removes confirmation.
Wake edits never change source timestamps. Missing/invalid timestamps in a v3
Shown entry are corrupt, not an invitation to manufacture evidence at startup.
Future-dated confirmation is stale under M1's clock rule; configuration still
governs admission. Source preferences have no TTL and survive re-pair to the
same saved-TV identity, without claiming proof for a replacement panel at the
same address.

Relax `CapabilityFact::valid` only for Hdmi1–3/Tuner with `supported`,
`user_confirmation`, a positive timestamp, no context, and current/stale
freshness. All other combinations keep M1's rules. The native decoder has
separate current-fact readiness, startup/admission and common-session checks;
update all of them to distinguish durable configuration from session evidence.
A durable fact can be current while disconnected, not selected or admission
closed; summary reasons still prevent that fact from granting action
eligibility. `evidence_epoch` is present if and only if at least one fact has
session context. A context-free app/IME fact, an unsupported durable source
fact, and a mixed or wrong session must still fail closed. Every other key
keeps its session rule. M2 overlays all four source facts from configuration
after the M1 in-memory projection, including unknown for Hidden/Unconfigured.
Historical session source observations must not revive a hidden key; session
fences cannot clear a durable preference. A send result never changes a fact.

**Labels.** At most 24 Unicode scalars and 64 UTF-8 bytes, trimmed and
non-empty. Reject control and bidirectional-override characters instead of
stripping them, because the label is user input, not TV data. Trim Unicode
whitespace before checking both limits; an omitted label means use the port
name, while an explicitly empty/whitespace-only label is invalid. Define the
forbidden set as Unicode controls U+0000–001F/U+007F–009F and bidi format
controls U+061C, U+200E–200F, U+202A–202E, U+2066–2069. Do not remove ordinary
combining marks or emoji joiners. Store the trimmed spelling without case
folding or normalization. The server validates every label. Clients pre-check
only for messages. Display names include the port identity even when labels
collide; a label is never an action identifier or MCP instruction.

## Storage

Store the configuration in the existing `DiskDevice` record as an optional
`sources` object. Forget already deletes that record inside its recovery
transaction, so M2 needs no sidecar and no new journal stage.

- Bump `DEVICE_VERSION` to 3. Reading v1 and v2 gives unconfigured sources.
  Older builds report the v3 file as `Corrupt`; they do not silently drop it on
  their next save. This fail-closed downgrade is the explicit choice.
- Before **any** first v3 write (save, select or delete), durably preserve the
  validated v1/v2 file as `devices.pre-v3.json` in the same private directory.
  Preserve its actual version; a v1 file must not be named v2. Use the existing
  private-file envelope/permissions and atomic durable-write boundary, under
  the repository lock. Backup failure blocks migration before replacing the
  live file. A fresh profile needs no backup. A retry reuses a verified
  matching backup; an invalid, conflicting or unsafe existing destination
  blocks the write instead of being overwritten. Test failure before/after each
  rename and acknowledgment, including successful rename with failed fsync.
- The backup is a historical recovery artifact and can contain records later
  forgotten. Document its path and retention/removal in `docs/architecture.md`;
  never load it automatically or imply that Forget erases historical backups.
  Downgrade is an explicit offline rollback with the owner stopped and recovery
  journals settled. It discards post-backup edits and can restore forgotten
  records; it is not a safe merge with current credentials/trust. Document
  coordinated recovery of those stores before recommending a restore.
- `pairing_journal.rs` serializes `PairingBackup.device` field by field (its
  `wake` block). Add `sources` there, or re-pair recovery restores a TV with
  its source configuration lost. Missing in an old backup means unconfigured.
- Missing `sources` in v1/v2 and old pairing backups means unconfigured.
  Present malformed v3 data fails closed, including unknown/duplicate keys,
  invalid states, labels on non-Shown entries and invalid timestamps. Bound
  disk parsing and test 1,024 fully labelled TVs against the private-file read
  limit separately from the HTTP limit. Version the pairing backup envelope
  when writing source-bearing backups so old recovery code cannot silently
  discard the new fields; retain reading of legacy backups.
- Re-pair clones the saved record and changes only label and host
  (`runtime_setup.rs`), so source configuration carries over just as Wake does.

## Mutation and admission

`Command::SetSources { target, generation, revision, configuration }` copies
`SetWake`. It requires the selected target, the current selection generation,
the per-TV config revision and an operation key, and it bumps the same
revision. Apply the existing owner epoch and storage-lane guards too. Reusing
an identical operation key/payload returns its retained receipt with no new
write; a different payload with that key returns `KeyConflict`. Never reject an
identical retry merely because its successful first write changed revision. Use
a full replacement with exactly one entry for each configurable key;
Unconfigured is an explicit reset, and SourceMenu is not configurable. Reject
missing, repeated and unknown entries before any disk effect. A source edit
therefore makes a concurrent `set_wake` stale, and the reverse is also true;
this is intended. `storage_write` currently takes `Option<WakeConfiguration>`,
with `None` meaning Select. Replace that with a typed `StorageWrite::{Select,
Wake, Sources}` rather than adding another overload. Carry the write kind
through completion and uncertain-write reconciliation too: the current error
path compares only `old.wake != new.wake`. Compare the sources and timestamps
for source edits, reread authoritative storage before releasing the lane, and
advance the shared revision when a write committed despite a lost
acknowledgment. If reread fails, block further edits until reconciliation;
never publish a speculative configuration or silently retry a timed-out write.

Source keys reach the runtime as `Command::Remote`. In `queue_remote`, before
`admit_remote`, an HDMI or Tuner action whose current configuration is not
Shown is rejected as `not_sent`, distinguishing `SourceUnconfigured` from
`SourceHidden` across HTTP, native and MCP reason mappings. Preserve the
existing selection, pairing, connection, queue and storage-busy guards;
configuration is an extra condition, never an alternative admission path.
Source keys never take the PowerToggle→Wake branch, never auto-connect and
never repeat. The check uses configuration at admission time; a later edit does
not cancel a queued key. State this in the editor: hiding prevents new
requests, but does not recall one already queued. Selection/session changes
retain their existing cancellation fences. Each explicit press is one finite
click with no repeat/hold mode.

## Projections

**Size.** The 1,024-TV profile is 4,071,889 of 4,194,304 bytes, about 119 bytes
per TV before event framing. That baseline has little headroom; M2 removes some
source-session metadata, so only a new measurement can establish the final
size. Use a bounded detail projection:

- Snapshot and SSE entries for TVs that are not selected add nothing. Their
  eight-fact array reflects Shown preferences (`supported`); it cannot
  distinguish Hidden from Unconfigured or recover a label. The M1 profile
  measured eight supported facts per TV, with session counters that source-key
  facts no longer carry.
- The full `sources` block (state and label per key) appears only in the
  selected TV's snapshot entry, in device detail and in `inspect_tv`. Editing
  requires the TV to be selected, so the GUI always has it.
- Re-measure the adversarial profile with the selected TV at maximum labels. If
  it does not fit, stop and ask for an owner decision; do not raise the limit
  or truncate.

Omission is deliberate projection, not data loss. Keep source details in a
separate optional client projection rather than materializing unselected saved
records with empty configuration. The selected snapshot entry and every detail/
`inspect_tv` response include all four entries and their shared config
revision; recovery-only targets have no invented configuration. Absence in a
compatible older response defaults to unavailable/unconfigured for display, but
must never be written back without a fresh editable detail and revision. A
present malformed block is an error, not the older-response fallback. Selection
changes discard or explicitly preserve a target-bound draft; they never
retarget it.

**HTTP.** `PUT /v1/devices/{id}/sources` mirrors `/wake` in
`control_http/transport.rs` and `request.rs`. Remote actions accept the five
new names. `control_client/projection.rs` encodes and decodes both. Include
sources in command fingerprints, operation receipts and rejected-reason
decoding; validate stale epoch/generation/revision, unknown actions and
malformed replacement bodies with zero storage/TV effects. Match existing
request-byte and lossless decimal-string counter conventions. Successful saves
publish configuration, facts and revision atomically in one ordinary
snapshot/SSE revision. No read performs a probe or write.

**MCP.** Add the five names to `remote_action`'s `action` enum in `catalog.rs`
and add the inspected TV's `sources` to `inspect_tv` (`tools.rs`, `results.rs`,
result corpus). `list_tvs` stays compact. Whether MCP may edit labels is an
open owner decision. The recommended default is GUI-only settings editing,
while MCP reads configuration and sends stable port actions. Editing through
HTTP remains available to the authenticated GUI role; decide and test the
route's client-role authorization explicitly. Reusing an existing MCP tool
would still require a separate schema/policy decision and budget measurement.
Measure the catalog and the result bytes, then pay for the growth with cuts or
record an owner re-baseline before shipping.

## GUI

- **Sources view** (`view.rs`, which replaces `empty_primary_view`): one button
  per Shown key, labelled with the user label or port name. The Sources menu
  button is always present. Show the canonical port alongside any custom label
  so two identical labels remain distinguishable. Without a selected or
  connected TV it shows the reason. A result reads "Source request sent";
  `uncertain` reads as such and offers no automatic retry. A link opens
  Settings when no key is Shown.
- **Settings**: per-key show/hide and an optional label for the selected TV,
  saved with `SetSources`. `workflows.rs` treats an uncertain or stale
  `SetSources` like `SetWake`: refresh, re-read the revision, and never
  resubmit silently.
- **Accessibility**: Tab order across buttons and editor fields; VoiceOver
  names such as "Request PlayStation, HDMI 2"; every disabled control states
  why.
- **Editor behavior**: configuration editing is available for a selected saved
  TV while disconnected, subject to startup/storage/recovery guards; sending
  still requires a ready paired connection. Distinguish default Unconfigured
  from explicit Hidden, provide reset, and require explicit Show confirmation
  explaining that it offers a request without detecting a port.

## Touch-point map

| Area | Files | Tests |
| --- | --- | --- |
| Domain | `domain/remote_action.rs`, `domain/capability.rs`, source configuration value types | Enum sets, `LIVE_ACTIONS` unchanged |
| Codec | `infrastructure/samsung/codec.rs`, `samsung/diagnostic.rs` | Key mapping, diagnostic reuse |
| Application | `device_repository.rs`, `control_service.rs`, `runtime.rs`, `capabilities.rs` | Admission, revision sharing, fact projection, validity |
| Storage | `infrastructure/preferences.rs`, `pairing_journal.rs` | v1/v2→v3, backup faults, disk bounds, Forget, versioned re-pair recovery |
| HTTP | `control_http/request.rs`, `transport.rs`, `response.rs`, `control_client/projection.rs` | `tests/control_http.rs`, `tests/capabilities.rs`, `tests/control_response.rs`; framed size profile |
| MCP | `mcp/catalog.rs`, `tools.rs`, `results.rs` | Catalog bytes, `inspect_tv` corpus, HTTP agreement |
| GUI | `presentation/iced/view.rs`, `view_model.rs`, `workflows.rs`, Settings page | `client_tests.rs` workflows |

Use the existing fake gateway and session. No test sends to a real TV.
