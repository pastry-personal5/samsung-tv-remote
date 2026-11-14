# P2-M11 acceptance review — 2026-11-14

Status: Active; not accepted as Done.

Implementation adds executable publication exclusion, combined native/MCP
fixtures, removal/recovery integration and user/developer guidance. The
[ledger](milestone-11-implementation.md) identifies source, topology, exact
test names and predecessor dependencies. Existing M9/M10/Phase 3 working-tree
changes are preserved. No owner installation, TV, credential or Claude
configuration has been changed by these automated fixtures.

## Overview checklist audit

| Completion item | Evidence delivered | Remaining acceptance |
| --- | --- | --- |
| Predecessors/reliability | Stable service boundary and delegated baseline checks; ownership and helper source review | M9/M10 native/client gates remain with their original owners. Historical helper/ownership failures have no diagnosed cause; D8 remains blocked. |
| Shared client state/admission | Four combined native encoder/transport/projection + MCP tests; existing ordering, pressure, storage and Iced fake-client suites | Complete mixed-load/interleaving audit; signed combined clients. |
| Private recovery/no replay | Paused write, discarded native receipt, original-key lookup, changed payload conflict, principal isolation and exact effect counts; existing physical lost-reply/expiry/epoch cases | Installed-client refresh, crash and uncertainty review. |
| Lifecycle/install | Process-lifetime executable guard, exclusive old/staged publication, post-quarantine legacy scans and retained rollback regressions | Final signed cold/concurrent launch, sleep, crash, compatible/incompatible upgrade and rollback N1–N4. |
| Removal | Running/stopped × unchanged/changed-entry process matrix; durable disable, helper denial, original-key retry, sticky Stop and explicit Resume with grant still disabled | Full Connect/helper/crash interleaving audit D9 and hands-on N11 with session consent. |
| Data preservation | No schema changes; namespace, retained-bundle rollback, saved synthetic TV and Stop/enrollment fixtures | Signed API/TV credential continuity and rollback N4/N6. |
| GUI boundary | Mandatory boundary checker/negative corpus and existing Iced fake-client tests; all new process policy is server-side | Final accessibility review does not follow from a static boundary pass. |
| Model budgets | Installed version 2.1.325 recorded; catalog unchanged; proxy and historical measurement limits documented | Authenticated isolated model-visible definition/result/call/turn measurements N8, both search modes and nine workflows. No owner budget re-baseline exists. |
| Prompts/credential states | Existing explicit blocked-state/peer/bootstrap fixtures; native procedure prepared | Zero routine prompts and native denied/locked/interaction-required observations N6. N7 separate-host lane waived, not passed. |
| Accessibility/hardware | [Runbook](milestone-11-native-runbook.md) prepared for keyboard, VoiceOver and target TV | N9 keyboard review, unscheduled VoiceOver, physical observations N10. No hardware response is inferred from fake writes. |
| Artifact/checks/docs | Current architecture, contribution/rollback/removal/troubleshooting guidance, source ledger and delegated validation | Final signed artifact/native execution and all unwaived gates above. |

## Deterministic and native row status

D1–D7 and D9–D12 have named deterministic coverage in the ledger; this is not
blanket acceptance of their complete matrix. D5's full mixed-class schedule
and D9's complete crash/Connect/helper schedule remain open. D8 is blocked on
diagnostic reproduction and an explained reliability resolution. A subsequent
green test run does not erase its historical failures.

N1–N4, N6, N8's actual-model lane, N9, N10 and N11 are **blocked** pending
observed evidence on the final candidate and the required session/topology.
N5 is **waived**: deterministic cross-UID and per-user runtime-directory tests
substitute, with real session switching unobserved. N7 is **waived**:
same-Mac protocol fixtures and future N10 observations substitute, with native
attribution and deny/revoke/restore unproven. N8's disposable-account production
helper subrow is **waived**: the required signed isolated corpus harness and
development helper tests substitute, with a fresh-account production identity/
Keychain path unobserved. The signed corpus portion of that substitute is still
pending. These waivers come only from the [owner decision](phase-2.md#owner-decisions).

The [native runbook](milestone-11-native-runbook.md) gives setup, action,
observable pass conditions and cleanup. N11 is ready for session-specific
approval and ends with Resume → Connect → Verify. No automated removal of the
owner's bundle has been attempted.

## Validation record

Delegated baseline formatting and Clippy passed. A sandboxed baseline full run
failed five filesystem/socket fixtures with permission errors. The full suite
then passed using the documented isolated socket/process permissions: 199 unit
tests and all integration targets. These baseline passes preceded M11 changes.

Focused M11 validation passed four cross-client tests, two publication unit
tests and 21 Python installer cases, plus formatting and all-target Clippy.
The first removal-matrix run failed because the test required success from
Stop on an already intentionally stopped owner. The fixture was corrected to
expect `server_stopped` in that branch; production Stop semantics were retained.
The corrected matrix then passed focused and full execution. Final delegated
checks passed:

- `cargo fmt --all -- --check`.
- `cargo clippy --all-targets -- -D warnings`.
- `cargo test` with documented isolated socket/process permissions: 201 library
  tests and every integration target, including all 19 server-process tests.
- `python3 -B tests/test_macos_bundle_install.py`: 21 cases.
- Four combined native/MCP tests, two publication unit tests and the extended
  mutation-saturation/recovery test passed focused execution.
- `git diff --check`.

`make bundle-candidate` succeeded and retained
`target/.m9-candidate.Il9XaQ.noindex/Samsung TV Remote.app`.
The candidate's identifier is `dev.samsungtvremote.local`, TeamIdentifier
`82W8N225JJ`; executable SHA-256 is
`3a8aea386b54e6d6cf1cb804b19ddf2a44bca6e505fa86c1523150cbb34395b5`.
Its `--help` mode exited zero with no stderr. No GUI/server/helper production
mode was launched and the candidate was not installed. This is artifact and
entrypoint evidence, not N1–N11 acceptance.

The signed candidate also refused `--help` while Python held an exclusive
executable lock: exit 1, empty stdout, exact sanitized
`{"error":"bundle_replacement_in_progress"}` stderr. After release it exited
zero again. The final 180-file source manifest SHA-256 is
`59bf1308d1b1450534ffc8c7daa96476b37a4a8cabd2b494dd945105cf4438af`;
the [ledger](milestone-11-implementation.md#candidate-and-entry-freeze--2026-11-14)
defines the reproducible manifest algorithm.

## Unattended validation follow-up — 2026-11-14

The [automation inventory](automated-validation.md) identifies Phase 2 checks
that need neither hardware nor human interaction. On base revision
`90b6552e6f1a5ba86b48d663656f46424a40d407` plus this working-tree diff,
three new regressions cover mixed native/MCP pressure, ownership during a
paused persistence worker, and crash recovery between durable MCP disable and
configuration removal. The eight Python synthetic TLS/WebSocket fixture
self-checks now also run from a Cargo integration target.

Focused execution passed all three new regressions and `cargo test --test
tv_fixture` (eight Python cases). Formatting, all-target Clippy and
`git diff --check` passed. The first sandboxed mixed-load run failed to bind
its ephemeral listener (`Operation not permitted`); the permitted isolated
loopback run passed. The first crash regression attached before foreground
startup completed and failed with `Bootstrap(Unavailable)`. Synchronizing on
ownership and using the launcher's bounded attach path corrected that fixture;
focused and full-run execution then passed.

The default-concurrency `cargo test` run **failed** in the previously recorded
`concurrent_header_helpers_cold_start_one_owner_and_reuse_one_mcp_bearer`:
one helper exited 1 with `{"error":"mcp_helper_timeout"}`. All 202 library
tests and preceding integration targets passed; the process target passed 19
of 20 cases, including both new transport/process regressions. Cargo stopped
at that failure, so later targets were not executed by that invocation.
The newly registered TV fixture passed separately. A focused concurrent-helper
rerun passed; that does not resolve D8 or invalidate the default-run failure.
The timeout suggests load sensitivity but does not establish its root cause.
Production deadlines and assertions remain unchanged.

The diagnostic full run `cargo test -- --test-threads=4` then **passed**:
202 library tests and every integration target, including all 46 HTTP cases,
all 20 process cases and the new TV fixture target (eight Python cases).
This is a qualified pass at four test threads, not a pass of the default
concurrency gate. It establishes neither the cause nor resolution of D8.

This follow-up is deterministic evidence only. The full D5/D9 schedule audit,
D8 reliability diagnosis and all unwaived native gates remain open. The
owner's installation, Claude configuration, credentials and physical TV were
not changed.
