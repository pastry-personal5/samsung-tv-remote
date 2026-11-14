# P2-M10 implementation review — 2026-11-13

Scope: current development implementation, including shared HTTP dispatch,
MCP framing, projections, enrollment, helper and configuration coordination.
This is a code review and repair record, not milestone acceptance. M10 remains
Active; the overview checklist and implementation ledger own delivery gates.

## Findings repaired

1. **Connect recovery rejected its own saved configuration.** After saving the
   token-free entry, a lost grant reply left the caller's original expected
   entry (often absence) different from the file. Retrying the same intent then
   failed before protected enrollment could recover its result. The writer now
   accepts an exact match with the desired entry as a byte-preserving no-op.
   Different entries still conflict, removal still compares the expected entry,
   and enrollment still fences stale generations after Disconnect. Isolated
   integration coverage retries the original generation/key/expected absence
   and confirms that the same retry cannot regrant after Disconnect.
2. **Duplicate-member rejection discarded a valid correlation ID.** Recursive
   unique-member decoding correctly rejected the request, but always omitted
   its ID. A rejection-only pass now consumes the complete JSON document and
   retains a single valid root ID. Multiple root IDs, including equal values,
   invalid IDs and malformed documents never supply a guessed ID. This pass
   performs no tool decoding or service admission. Protocol fixtures cover
   nested duplicates and ambiguous, numeric and malformed cases.
3. **Inspection bypassed the documented label preview bound.** `list_tvs`
   truncated labels while `inspect_tv` returned the full native device label.
   Inspection now uses the same 80-Unicode-character preview with an explicit
   `label_truncated` flag and corresponding output-schema constraints. A shared
   listener test validates multibyte labels against the published schema and
   checks that the native API still returns the complete saved label. Hosts,
   device IDs and configuration guards retain their exact values.

## Remaining acceptance limits

The existing ledger correctly keeps GUI Connect/Disconnect integration,
installed-client scope/capability and lock preflight, signed helper/client
acceptance, real model-visible token budgets and native permission evidence
open. Configuration editing must remain gated until its required preflight is
implemented and verified. The earlier intermittent concurrent cold-start
helper assertion also remains unresolved; these repairs do not establish its
root cause. No owner configuration, credential, installation or TV is changed
by this review's automated fixtures.

## Validation

Delegated formatting and Clippy passed. The first full run failed the existing
`admitted_management_retains_profile_ownership_after_composition_guard_drops`
assertion that a lock could be reacquired immediately after dropping enrollment
(185/186 unit tests passed). Its focused rerun and the next full run's unit
suite passed; no root cause is established. Keep this as a reliability follow-up.

The next run exposed an invalid new label fixture: its 420-byte saved label
exceeded the service's existing 256-byte restoration bound. The fixture now
uses 90 Unicode characters / 180 bytes and explicitly checks successful startup.
Final delegated validation passed `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings` and default-concurrency `cargo test`:
384 tests passed, including 186 unit tests, 36 shared HTTP tests, three connection
tests, nine protocol tests and 18 server-process tests. The earlier ownership
assertion did not recur. No production or installed-client acceptance is
inferred from automated tests.

## Follow-up implementation review — 2026-11-14

Two additional defects were repaired in the current M10 working tree:

1. **Rollback could discard a second concurrent configuration writer.** After
   detecting a publication race, the writer checked that its document was still
   at the destination, then swapped the displaced document back and unconditionally
   deleted the temporary file. Another writer between that check and swap could
   therefore lose its settings. Rollback now verifies both inode identity and
   exact bytes of the displaced file before cleanup. A different or unreadable
   document remains named for recovery and is reported through the existing
   partial-progress error. Deterministic checkpoints exercise both in-place edits
   and atomic replacement at this interleaving.
2. **Enrollment last-key recovery did not compare the full intent.** A retained
   key with the same enabled flag but a different expected generation incorrectly
   returned success. Each durable transition advances generation exactly once,
   so recovery now compares the supplied expected generation with the retained
   generation minus one. Changed intent returns `operation_key_conflict` without
   modifying state. Tests cover both enable and disable, including maximum-counter
   input, while preserving exact original-request recovery.

Existing edits are preserved. Signed/native/client acceptance, actual-model
budgets and the previously intermittent ownership/helper failures remain open;
these repairs do not establish their cause or accept the milestone.

Delegated validation passed both focused regressions, `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings`, `git diff --check` and the full
`cargo test` suite: 199 unit tests and all integration suites passed, including
18 server-process tests, with zero failures and exit status zero. The full run
used the documented isolated loopback/process permissions after the sandboxed
run encountered permission errors. The known intermittent failures did not
recur; a passing run does not establish their root cause.
