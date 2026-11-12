# P2-M7 completion review

Date: 2026-11-12. Status: Done under the recorded entry exception.

## Evidence and limits

The application runtime and temporary GUI adapter are implemented. Required
validation is `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings`, and `cargo test`.
The full suite passes 174 tests (98 unit, 76 integration), including 33
headless runtime, ten Pairing/recovery and seven Forget/recovery cases.
The entry exception leaves M6's signed credential and process-choice gates
open. No real TV, production Keychain migration, signed native app review,
HTTP/MCP server, background launch or physical-state observation is claimed.

## Follow-up code review

The review reproduced and fixed three gaps with deterministic regressions:

- **P1 — Pair commit did not settle the owner's concurrent Wake.** Revoking
  only the coordinator permit left the operation and Wake handle pending;
  a late send completion could re-enter reconnect handling after Pair changed
  the session. Commit now cancels through the owner, retains send uncertainty,
  and removes the live Wake handle. Covered by
  `pairing_commit_settles_concurrent_wake_and_revokes_pending_packet`.
- **P2 — Terminal connection paths left the snapshot Connecting.** A storage
  deadline, rejected preparation during a configuration fence, explicit Wake
  cancellation, or supervisor failure could settle the operation without
  clearing its connection projection. These paths now clear only the matching
  current Connecting state, preserving newer generations and Ready sessions.
  Covered by `connection_storage_deadline_clears_connecting_before_worker_returns`,
  `cancelled_connection_during_configuration_commit_clears_connecting_projection`,
  `cancelling_wake_during_reconnect_clears_connecting_projection`, and
  `connection_supervisor_failure_clears_connecting_projection`.
- **P1 — Successful preference writes ignored profile reread failures.**
  Select and SetWake could report Ready after their authoritative reread found
  corrupt storage. Every write completion now reconciles the bounded profile;
  an unreadable profile visibly blocks subsequent admission. Same-ID reselect
  still advances generation exactly once. Covered by
  `successful_preference_writes_still_validate_the_authoritative_profile` and
  the existing selection/revision tests.

## Production resource bounds

| Resource | Limit and behavior |
| --- | --- |
| Ingress | 128 requests, immediate typed Busy if full; dedicated drain signal. |
| Completions/session | Eight joined effect completions independent of ingress; 32 session events; ingress opportunity every 16 owner transitions. |
| Non-remote effects | Eight total, including timed-out blocking workers; discovery/probe starts conservatively require fewer than two existing workers. One Pair preparation and one serialized storage lane. |
| Remote FIFO | 32 waiting plus one writer; 33 internal journal slots drained into the independently retained registry. |
| Registry | 32,768 total; 8,192 retained and 64 pending per client. No early result eviction. 30-minute monotonic terminal retention. |
| Subscribers | 64 total, four per client, 64 events each; full queue closes and requires resnapshot. |
| Candidates | 32 per discovery; 1,024 live globally and 64 per client; two-minute lifetime. IDs bind client, epoch, discovery result and host. |
| Observations | 128 live globally, eight per client, five-minute lifetime. Starts reserve conservative space including current workers. Retained outcomes do not extend handles. |
| Input/data | Keys 128 bytes; hosts 253; Pair labels accepted at 80 bytes, input envelope at 256; safe name/model fields 256 each; saved labels 256; 1,024 saved records. Oversize inputs/profiles fail explicitly. Files remain capped at 1 MiB. |
| Persisted recovery | One cross-store marker and one non-secret metadata/trust backup slot; no secret file. |
| GUI client | 64 submitting/pending requests; one runtime event subscription, local candidate list at most 32 and one displayed observation. |

`tests/runtime_limits.rs::production_saturation_memory_and_stage_timings`
uses a counted System allocator in a dedicated test binary. It fills the
production registry with maximum-bound Pair inputs/policy results, the full
1,024-device profile (maximum host/label lengths), 1,024 candidate handles,
128 observations, and 64 subscriber queues. The final queues also retain 21
changing full-profile versions, not just a shared unchanged profile. All
32,768 results remain queryable, duplicate admission still works and a new
key refuses capacity without an effect. Peak heap has a 128 MiB regression
ceiling for this fixture, not a production RSS promise or a cap on copies that
an external caller elects to retain after receiving them.

Measured on this macOS arm64 development machine, unoptimized test build:

| Stage / quantity | Measured sample |
| --- | --- |
| Live / peak incremental tracked heap at joint saturation | 68,632,003 / 69,218,971 bytes (about 65.5 / 66.0 MiB) |
| Restore maximum fake profile | 1,941 µs |
| 32 fake discovery operations | 8,493 µs total |
| 128 fake probe operations | 15,399 µs total |
| 32,586 maximum-input registry admissions | 1,645,940 µs total |
| Snapshot query at saturation | 18 µs |
| Idle-effect drain at saturation | 11,547 µs |
| First Pairing, fake immediate approval plus real temporary-file persistence and owner round trips | 78,376 µs |

Reproduce the measurements with `cargo test --test runtime_limits -- --nocapture`
and `cargo test --test pairing_runtime first_pair_and_repair_commit -- --nocapture`.
Values vary with scheduling and filesystem load; they do not estimate actual
Samsung, Keychain, Local Network consent or user-approval latency.

## Deadline decisions

| Workflow | Final M7 supervision |
| --- | --- |
| Restore / each storage stage | 10 seconds to visible blocked/terminal state; an actual blocking call retains its slot/fence until return. |
| Discovery / probe | 10 / 20 seconds; existing adapter's SSDP 5-second window and TLS/metadata stages remain unchanged. |
| Pair / re-pair | At most 80 seconds preparation, then a 10-second commit reporting deadline (90 total); actual transport's 8 + 8 + 45 stages remain unchanged. |
| Ordinary Connect | 75 seconds from admission including material read and possible rotation; network stage also capped at 65 seconds. Store calls separately capped at 10 seconds and retain late fences. |
| Disconnected Power check | Two seconds from admission, including material read. A late storage call remains fenced and cannot be treated as an offline TV or trigger a packet. |
| Packet send | 10-second reporting deadline; revoke the permit and report send uncertainty, retaining the underlying worker rather than dropping a blocking job. |
| Wake readiness | 30 seconds from reconnect start, including material read/rotation; eligible network retry delay is 500 ms, no packet replay. |
| Remote | Four-second adapter write; 136 seconds total admitted lifetime including FIFO residence. Deadline abort clears connection/inference and preserves write uncertainty. |
| Drain | Five-second report; late workers settle separately. |

Store bounds are caller-visible reporting deadlines, not claims that Keychain
or a filesystem call can be killed. Fake-time tests advance explicit deadlines;
no deadline justifies replay or concurrent replacement writes.

## Verification matrix

All names below are deterministic automated tests. `headless_runtime` and
`pairing_runtime` run without initializing Iced; GUI tests invoke update and
projection functions without launching a windowing event loop.

| Case | Test/evidence reference |
| --- | --- |
| Headless parity/restore | `headless_runtime::restore_preserves_duplicate_hosts_and_generation_zero_without_tv_io`, `corruption_blocks_admission_visibly_and_retains_rejection`; preference v1/v2/trust round-trip unit fixtures; `pairing_runtime::first_pair_and_repair_commit_before_generation_and_adopt_only_the_committed_session`. |
| Admission and keys | `duplicates_precede_capacity_and_changed_payload_conflicts`, `per_client_retention_quota_deduplicates_and_leaves_space_for_another_identity`; runtime unit `dropped_reply_does_not_remove_admission`. |
| Two-client selection | `two_clients_cas_selection_and_same_id_reselection_changes_generation`, `ambiguous_selection_and_configuration_failures_reconcile_actual_durable_facts`. |
| FIFO/inference/stale sessions | `headless_session_pumps_remote_and_inference_without_gui_polling`, `drain_retains_uncertain_writer_and_not_sent_waiters_without_replay`; coordinator tests for stale/duplicate session events, token rejection and dispatch-time Play/Pause. |
| Power and Wake | `disconnected_power_reachable_is_ready_without_key_or_packet`, `disconnected_power_offline_sends_exactly_one_packet_and_cancel_preserves_facts`, `wake_captures_configuration_and_edits_do_not_cancel_or_replace_its_packet`, `wake_cancel_before_final_send_and_send_deadline_revoke_the_packet_permit`. |
| Setup/rotation/freshness | `pairing_runtime::late_approval_and_token_rotation_cannot_mutate_a_new_selection`, `observation_is_rechecked_after_approval_before_any_store_commit`, `reconnect_rotation_is_journaled_and_preserves_selected_generation`; `headless_runtime::prepared_connection_cannot_install_inside_an_uncommitted_selection_fence`. |
| Crash recovery | `forget_recovery::every_forget_journal_boundary_recovers_without_implicit_credential_access` (12 boundary stops); `pairing_runtime::every_pairing_and_repair_commit_boundary_blocks_on_restart_and_recovers_explicitly` (24 stops); `every_first_pair_compensation_boundary_resumes_only_incomplete_steps` (10 stops). These model process interruption, not physical power loss. |
| Blocking stores | `timed_out_blocking_worker_keeps_fence_and_queries_responsive`, `pairing_runtime::timed_out_pair_commit_stays_fenced_through_drain_and_late_success_never_installs_a_session`. |
| Events/privacy | `subscription_is_atomic_overflow_closes_and_client_results_are_private`, `pairing_runtime::observations_are_client_bound_exact_expiring_and_bounded_independently_of_result_retention`. |
| Retention/restart/identity | `terminal_retention_is_monotonic_and_restart_is_explicit`, `operation_id_lookup_is_client_scoped_retained_and_epoch_bound`, `per_client_retention_quota_deduplicates_and_leaves_space_for_another_identity` (new handle with stable identity models credential refresh). Real credential validation/revocation belongs to M8. |
| Saturation/fairness/deadlines | `production_saturation_memory_and_stage_timings`, `completions_and_queries_progress_while_client_ingress_is_saturated`, `subscribers_and_effects_have_finite_caps_and_drain_closes_clients`, `discovery_deadline_releases_effect_capacity`, `effect_panic_settles_original_reservation_and_owner_survives`, `client_candidates_expire_without_extending_retained_discovery_results`. |
| Drain/detach | `discovery_progress_survives_client_detach_and_duplicate_has_one_effect`, all five `headless_runtime` drain cases, and the late Pair commit test above. |
| Temporary GUI parity | `app::tests::gui_launch_connect_is_explicit_once_and_does_not_select`, `wake_drafts_survive_resnapshot_and_coalesce_while_storage_is_fenced`, `terminal_before_admission_reply_is_recovered_without_duplicate_feedback`, `cleanup_remains_actionable_after_partial_forget_removes_selected_record`; retained Settings/navigation/shortcut tests and view/model tests. Buttons and shortcuts map to the same service requests; no session/storage completion variants remain in UI messages. |

M8 can wrap this service with a transport. Production profile ownership and
signed behavior still require their recorded M6/M9 gates; no simultaneous old
and new owner may open one profile.
