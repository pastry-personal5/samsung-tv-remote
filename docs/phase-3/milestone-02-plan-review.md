# P3-M2 plan review — 2026-11-14

Status: Active — plans corrected; implementation and acceptance gates open.

Reviewed the [overview](milestone-02-overview.md) and
[architecture](milestone-02-architecture.md) against the current working-tree
M1 foundation, runtime admission/storage completion, operation registry, device
repository, pairing journal, native decoder and capability size fixture.
Existing implementation changes were retained. This review changes plans only;
it authorizes no production implementation, TV effects or milestone completion.

## Findings and corrections

| Priority | Finding | Correction |
| --- | --- | --- |
| High | Relaxing `CapabilityFact::valid` alone leaves native current-fact readiness, startup and common-session checks rejecting durable source preferences. | Define the exact context-free source-fact exception and update every decoder fence, retaining session validation for apps/IME and summary reasons for eligibility. |
| High | The model uses a save timestamp that is not specified in storage; restart or relabelling could invent new evidence. | Persist per-Shown-key confirmation time, preserve it for label/Wake edits, clear it on hide/reset, and apply M1's future-time staleness rule. |
| High | Storage error reconciliation compares only Wake fields, so a source write committed despite a failed acknowledgment could retain a stale config revision. | Carry a typed write kind through completion, compare authoritative source state/timestamps, advance the shared revision, and keep the lane blocked when reread cannot reconcile. |
| High | A one-time v2-named backup does not cover v1 input, first writes through Select/Delete, durability failure or rollback of newer records. Old pairing-backup readers can also ignore new fields. | Specify a version-neutral pre-v3 backup at every migration write boundary, private atomic persistence and conflict/fault tests; version source-bearing pairing backups; document coordinated offline rollback and historical-backup retention. |
| High | Calling every repeated key a rejection conflicts with the existing receipt-returning operation registry. | Identical retry returns the retained receipt; changed payload conflicts. Both produce zero additional writes or sends. |
| Medium | Unselected snapshots deliberately omit source details, while the fallback decoder calls missing data unconfigured. That can erase configuration if reused as an editable record. | Keep optional detail separate from saved records, require fresh selected detail/revision for edits, reject malformed present data and never write omission back. |
| Medium | One `SourceNotShown` reason cannot satisfy the overview's distinct hidden/unconfigured reasons. Replacement-body shape and role authorization are unspecified. | Separate reasons, require exactly four configurable entries, reject duplicate/missing/unknown keys, and explicitly test the authenticated GUI route's role policy. |
| Medium | The hardware paragraph assumes every outcome is `written`, and a deferred matrix could be read as accepted. | Preserve actual request outcomes separately from panel effects; inconclusive/deferred cells leave acceptance open unless the owner narrows scope. |
| Medium | Label identity, reset behavior, disconnected editing and hiding an already queued key lack user-visible contracts. | Define exact Unicode/byte rules, canonical port labels, explicit Show/reset, disconnected settings guards and admission-time queue semantics. |

## Validation contract

The revised checklist requires deterministic fake tests for two-client shared
revisions, retry effect counts, uncertain-save reconciliation, migration
faults, versioned pairing recovery, durable versus session evidence, omitted
details, queue behavior and framed HTTP/SSE and disk limits. No hardware fact
is inferred from a fake test or a successful local write. M1's 4,071,889-byte
snapshot is a historical baseline, not a measurement of the proposed M2
representation.

Documentation validation passed: all 15 local Markdown file targets in the
four touched documents exist, whitespace checks pass, and `git diff --check`
passes. The revised plans were reread against the inspected code. Cargo checks
were not run for this documentation-only change. Implementation acceptance
requires the contribution-guide checks and the M2 fake/hardware evidence.

## Second review — 2026-11-15

Rechecked the corrected plans against `runtime.rs`, `control_service.rs`,
`response.rs`, `preferences.rs`, `pairing_journal.rs`, `samsung/diagnostic.rs`
and the MCP catalog/results. Plans only; no code changed.

| Priority | Finding | Correction |
| --- | --- | --- |
| High | The plan called a hidden-key rejection `not_sent`. `response.rs` maps admission `Rejection`s to HTTP error codes; `{"kind":"not_sent"}` only comes from a queued request's terminal outcome. | Top-level `Rejection::SourceUnconfigured`/`SourceHidden`, 412, like `WakeNotConfigured`. Fixed precedence: stale selection, then source configuration, then dispatcher guards. Matrix records the rejection code or terminal outcome. |
| High | `Command::metadata` and the `execute` selection fence end in wildcards, so a `SetSources` missing from either compiles and skips the stale-selection fence. | Name both sites; require a stale-generation `SetSources` test. |
| High | `storage_write` fences the session whenever the configuration is `None`. A naive typed replacement could disconnect on a source save. `Completion::Storage` bumps the first device with any Wake diff, not the write's target. | Only `Select` fences. Carry the write kind and compare only the target. |
| Medium | "Diagnostic reuses the codec mapping" would edit the frozen, feature-gated M1 controller after a signed candidate was prepared. | Leave `diagnostic.rs` unchanged; add a `p3-spike` agreement test; dedupe after M1 closes. |
| Medium | Wire spelling was unspecified, and `SourceMenu` maps to capability key `sources`, not `source_menu`. | Pin PascalCase action names and one domain action-to-fact map. |
| Medium | Nested `DiskDevice` has no `deny_unknown_fields`, so a conditional backup version still lets old readers drop `sources`. Wake's `serde_json::Value` keeps the last duplicate key. | Pairing backup version 2 unconditionally (read 1 and 2); a typed `sources` struct with `deny_unknown_fields`. |
| Low | MCP already edits Wake, so GUI source edits make pending MCP `set_wake` stale. | Document and test the `stale_config` → `inspect_tv` recovery; labels fit under MCP's 80-character limit, so no truncation. |
| Low | A configurable tuner departs from the phase wording ("ordinary keys") but was presented as settled. | Moved to the owner gate. |

## Remaining decisions

- An owner exception is required to implement M2 before predecessor acceptance.
- The tuner reading: configurable like an HDMI port (proposed) or always shown.
- GUI-only source editing is recommended; the MCP editing policy remains open.
- MCP enum/result growth needs measured offsets or an owner-approved
  re-baseline before shipping; keeping 15 tools does not establish budget
  compliance.
- Hardware evidence and any scope reduction remain explicit acceptance work.
