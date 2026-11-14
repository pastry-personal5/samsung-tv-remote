# P2-M11 implementation ledger

Status: Active — deterministic implementation underway; native acceptance open.

## Candidate and entry freeze — 2026-11-14

- Base revision: `592ad983d852784ff86f6610f5f1abf5d7a245de`, with pre-existing
  M9/M10 and Phase 3 working-tree edits preserved. This is a working-tree
  candidate, not a committed or accepted signed artifact.
- Final source manifest: 180 files, SHA-256
  `59bf1308d1b1450534ffc8c7daa96476b37a4a8cabd2b494dd945105cf4438af`.
  Hash sorted paths under `src`, `scripts`, `assets`, `tests` (excluding
  `__pycache__`/`.pyc`), plus existing `Cargo.toml`, `Cargo.lock`, `build.rs`
  and `Makefile`. Feed each UTF-8 relative path, NUL and binary file SHA-256
  into the manifest SHA-256. This includes untracked implementation files;
  documentation and ignored build products are excluded.
- Host: macOS 27.2.1, build 26B131. Installed Claude Code: 2.1.325.
- Protocol: native `/v1` plus MCP `2026-07-28` from
  `src/server/mcp/protocol.rs`; one literal-loopback listener and one owner.
- Deterministic topology: temporary profiles, ephemeral loopback listener,
  synthetic TV/session/Wake ports, synthetic credentials and isolated Claude
  configuration. No production profile override or owner credentials.
- Runtime defaults (`src/application/runtime.rs`): ingress 128, registry
  32,768, effects 8, subscribers 64 / 4 per principal, backlog 64,
  devices 1,024, observations 128 / 8 per principal, terminal retention
  1,800 seconds, storage deadline 10 seconds, drain deadline 5 seconds.
- Transport defaults (`src/server/control_http/transport.rs`): connections 128,
  ordinary requests 64 / 16 per principal, classification 4 / 2 per principal,
  mutation reservations 8 per principal. Header deadline 5 seconds, ordinary
  connection deadline 30 seconds, MCP deadline 14 seconds. Helper deadline
  8 seconds (`src/server/mcp/helper.rs`). Capacity expiry never proves no send.
- Storage/enrollment formats remain unchanged; no migration or credential
  replacement is introduced.
- Preserved Apple-signed candidate: `target/.m9-candidate.Il9XaQ.noindex/Samsung TV Remote.app`;
  identifier `dev.samsungtvremote.local`, TeamIdentifier `82W8N225JJ`.
  Executable SHA-256: `3a8aea386b54e6d6cf1cb804b19ddf2a44bca6e505fa86c1523150cbb34395b5`.
  `make bundle-candidate` and side-effect-free `--help` passed. It was not
  installed and no production profile was opened. Native installed-runtime
  acceptance remains blocked. Earlier bundle evidence is historical.
  Holding the executable's exclusive publication lock made signed `--help`
  exit 1 with empty stdout and only `bundle_replacement_in_progress` JSON;
  releasing it restored exit 0. This proves the actual signed early guard.

## Implemented changes

### Cross-client correctness

`tests/support/control_integration.rs`, included by `control_http`, uses the
native command encoder, bounded HTTP transport and typed receipt/snapshot
parser against the same listener/runtime as the MCP route. It adds:

- Captured native selection guards rejected after MCP changes selection;
  captured MCP configuration revisions rejected after native storage changes.
  Quiescent state comparisons require equal epoch/revision tags.
- A paused write with a discarded native receipt, MCP original-key recovery,
  native retained-result recovery, changed-payload conflict and exact effect
  counts. Equal key text under another principal grants neither lookup nor
  deduplication ownership; that principal's explicit intent is independent.
- Subscription attachment concurrent with MCP completion. Either the atomic
  snapshot already includes the terminal revision or consecutive public events
  reach it. Private result contents stay out of events.
- Partial native Forget with failed credential cleanup. Both clients see the
  recovery fence; a duplicate never retries deletion. Only an explicit MCP
  `recover_forget` completes the remaining cleanup.

The existing MCP mutation-saturation fixture now also verifies that dropping
stalled requests returns reservations and permits a subsequent MCP effect.

### Installation and rollback

`server/publication.rs` takes a nonblocking shared lock on a bundled
executable before mode dispatch and holds it for the process lifetime, including
GUI dispatch. No profile is opened to do this. Close-on-exec prevents a spawned
child retaining the parent's guard; the child obtains its own guard.

The installer verifies the stage, exclusively locks its executable, settles
the old owner and exclusively locks the old executable. An intervening
cooperating launch aborts replacement. Locks follow both inodes through rename,
verification and rollback. Legacy builds lack the startup guard, so additional
process checks after moving the old bundle detect late legacy launches before
publication and restore the unchanged old bundle. Arbitrary launches of a
retained legacy bundle remain outside this cooperating protocol; signed
old/new upgrade acceptance is still required.

Python regressions cover the check/rename interval, locks across rollback,
retry after failure and reinstalling the retained bundle while unrelated
synthetic data/Stop/enrollment files remain byte-identical. Rust tests cover
multiple process-lifetime shared guards and exclusive publication exclusion.

### Removal

`removal_disconnect_then_stop_preserves_disable_across_reopen_and_config_conflict`
exercises an isolated actual server in four combinations: running/already
stopped and unchanged/externally changed entry. It pairs a synthetic TV,
disconnects, retries a lost removal reply, retains changed entries, Stops,
verifies helper denial and sticky Stop, then explicitly Resumes. The saved TV
survives and MCP remains disabled. No new removal action, fence or state is
added. Moving the owner's bundle to Trash is a separate native N11 check.

## Predecessors and reliability

| Dependency | Current evidence | Status / next step |
| --- | --- | --- |
| M8/M9 stable client-only service boundary | Existing GUI boundary, client projection and transport suites | Available for deterministic integration. |
| M9 signed blocked credentials, Wake/visual and final artifact | [M9 audit](milestone-09-acceptance-audit.md) | Blocked; retain M9 ownership and run the final native candidate. |
| M10 helper/enrollment ownership intermittent failures | Baseline full suite passes with socket/process permissions; focused ownership runs pass. `try_clone` retains the open file description and admitted worker ownership. | Unresolved: no diagnosed historical interleaving. Successful reruns do not close D8. |
| M10 config publication/enrollment fault coverage | Existing exact-entry rollback, full-intent key and connection tests; new removal integration | Deterministic evidence only; does not accept native client workflows. |
| M10 actual-model budgets | Historical shipped catalog: 57,345 bytes / 14,337 byte/4 proxy units; previous isolated auth check had no login | Blocked pending authenticated isolated model session and visible-definition capture. Neither a CLI connection nor a byte proxy accepts the 1,900 gate. |

The possible transient inheritance of open descriptors before a concurrent
child reaches exec is a diagnostic hypothesis for immediate lock reacquisition,
not an established cause. Do not change lock lifetime or weaken the assertion
to conceal the historical failure. No reliability gate is waived here.

## Requirement-to-test inventory

The test runner owns command execution; the main thread owns repairs and
acceptance judgment. Existing test names below identify coverage, not automatic
acceptance of every interleaving in the broader matrix.

| Row | Named test / source | Evidence tier and remaining work |
| --- | --- | --- |
| D1 | `captured_native_and_mcp_selection_and_configuration_guards_never_retarget`; Iced `client_tests` | Combined deterministic transport plus fake presentation. |
| D2 | `attaching_gui_events_during_mcp_completion_cannot_lose_the_terminal_revision`; `atomic_events_are_consecutive_private_and_rotation_closes_old_streams`; `waiting_survives_an_overflowed_subscription_without_claiming_server_stop` | Atomic attach, ordered events, overflow and bounded private lookup fixtures. |
| D3 | `native_reply_loss_and_mcp_recovery_share_dedup_but_not_principal_ownership`; `losing_an_admitted_http_reply_does_not_cancel_or_repeat_the_remote_write` | Paused effect, discarded and physically lost reply, conflict and principal isolation. |
| D4 | `stale_epoch_reports_current_and_old_epoch_lookup_never_replays`; operation registry unit tests; `gui_client_reattaches_after_process_outage_without_replaying_setup_connect_or_action` | Fake-clock registry and isolated process restart; native crash remains N2. |
| D5 | `mutation_saturation_does_not_block_post_recovery_classification`; `queries_and_management_keep_reserved_capacity_under_slow_mutation_load`; `stalled_large_responses_keep_their_query_reservations_until_connection_close`; `runtime_limits` | Shared transport/runtime saturation and cancellation cleanup; full mixed-class interleaving audit remains open. |
| D6 | `partial_native_forget_is_shared_but_only_explicit_mcp_recovery_finishes_cleanup`; `pairing_storage_faults_over_http_compensate_or_expose_the_exact_recovery_marker`; `every_forget_journal_boundary_recovers_without_implicit_credential_access` | Real application policy with storage-fault ports; exact cleanup markers and no replay. |
| D7 | `gui_client_reattaches_after_process_outage_without_replaying_setup_connect_or_action`; `native_stop_is_epoch_fenced_sticky_for_all_clients_and_only_explicit_resume_reopens`; concurrent helper test | Isolated process/client evidence; signed combined GUI/MCP session remains N2. |
| D8 | `concurrent_header_helpers_cold_start_one_owner_and_reuse_one_mcp_bearer`; `admitted_management_retains_profile_ownership_after_composition_guard_drops` | Historical intermittent failures unresolved. Controlled diagnosis still required. |
| D9 | New removal integration; `mcp_offline_preparation_and_revocation_preserve_stop_without_constructing_a_runtime`; enrollment/connection unit fault tests | Running/stopped, conflict, lost-reply retry and helper denial. Complete Connect/helper/crash interleaving audit remains open. |
| D10 | `test_macos_bundle_install.py`; publication unit tests; `bundle_signing` | Fake process/signature/publication boundaries. Signed upgrade remains N4. |
| D11 | `profile_namespaces`, `development_profile`; `detached_parent_exit_sticky_stop_explicit_resume_and_fake_pairing_survive_restart`; new removal and retained-bundle rollback tests | No schema changes. Native Keychain continuity remains N4/N6. |
| D12 | `gui_boundary` including negative corpus; Iced `client_tests`; `control_client` / rotation suites | Mandatory static boundary and fake-client behavior. No second runtime or presentation policy introduced. |

For N1–N11 use the [native runbook](milestone-11-native-runbook.md). N5 and
N7 are waived only by the recorded owner decision; N8's disposable-account
subrow is waived, while actual model-visible workflows remain blocked. N9's
VoiceOver session remains blocked. All other native rows are blocked pending
an observed session on the final artifact.

## Validation

Baseline delegated `cargo fmt --all -- --check` and all-target Clippy passed.
The first sandboxed full test run failed five filesystem/socket fixtures with
permission errors. Repeating the full suite with the documented isolated
socket/process permissions passed all 199 unit tests and integration targets.
The initial Python installer baseline passed 17 cases. These are baseline
results, preceding the M11 changes above.

Final delegated formatting, all-target Clippy, full `cargo test` (201 library
tests and every integration target) and `git diff --check` passed. The full
process target passed all 19 tests, including the four-case removal matrix.
All four combined client tests, two publication unit tests and 21 Python
installer cases passed. Exact limits and remaining acceptance are recorded in
the [acceptance review](milestone-11-review.md).
