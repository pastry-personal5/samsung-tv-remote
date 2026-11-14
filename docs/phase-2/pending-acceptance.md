# Phase 2 pending acceptance tests

Status: Open. This checklist tracks unfinished acceptance for Version v2.0.0.
The version bump and release note do not close Phase 2. Existing automated
coverage is listed in the [unattended validation inventory](automated-validation.md);
the [M11 acceptance matrix](milestone-11-acceptance.md) defines the requirements,
and the [native runbook](milestone-11-native-runbook.md) provides execution steps
and cleanup.

The latest full-suite evidence predates the version bump: default concurrency
failed with `mcp_helper_timeout`, while the full four-thread run passed.
The Version v2.0.0 bundle-metadata regression passed after the bump using fake
signing tools. No final Version v2.0.0 signed bundle or native session has been
accepted. Keep earlier results as historical evidence tied to their source.

## Automated reliability and race acceptance

- [ ] **D5 — Complete mixed-load audit.** Existing tests cover both roles'
  stalled mutation bodies alongside SSE, POST reads, private waiting,
  management admission and cancellation recovery. Audit the remaining shared
  and per-role capacity combinations, including malformed bodies, slow
  responses and simultaneous pressure; add controlled schedules where missing.
  Pass when recovery/management stays available within its reserved capacity,
  reservations are released and rejected work causes no TV effect.
- [ ] **D8 — Diagnose and resolve helper/ownership intermittency.** The latest
  default-concurrency failure is in
  `concurrent_header_helpers_cold_start_one_owner_and_reuse_one_mcp_bearer`:
  a helper returns `mcp_helper_timeout`. Historical final-enrollment-guard
  release failures also lack an explained cause. Capture secret-free timing
  and ownership facts, establish the cause and add a controlled regression.
  Pass when one owner survives concurrent starts, locks remain held until
  admitted work finishes, delayed helpers cannot use superseded grants and
  the required default-concurrency validation passes. Focused/four-thread
  passes alone do not close this row.
- [ ] **D9 — Complete removal race/crash audit.** Existing tests cover
  running/stopped owners, changed entries, lost replies and a crash after
  durable disable before config removal. Audit concurrent Connect/helper
  startup and the remaining durable boundaries, including relocated/reinstalled
  helpers. Pass when disable and Stop persist, changed settings remain intact,
  no helper resurrects access and only explicit repair/Connect restores it.

## Signed macOS and installed-client acceptance

These rows remain **blocked** pending observed evidence on the final artifact
and the required topology. Use approved isolated fixtures for fault scenarios.

| Open row | Test to complete | Observable pass condition |
| --- | --- | --- |
| N1 | Cold and simultaneous signed GUI/helper launch; compatible attachment; unrelated listener and hostile/wrong-signature peers. | One authenticated owner, no manual port/token setup or routine prompt; unrelated peers fail closed. |
| N2 | Installed GUI plus configured Claude Code: close/reopen GUI, Stop/Resume, incompatible restart and crash recovery. | MCP survives GUI exit; both clients respect Stop; restart supplies a fresh epoch/snapshot; uncertain actions are not replayed. |
| N3 | Sleep/wake with an in-flight action and broken event stream; repeat with an intentionally stopped profile. | Bounded reconnect/resnapshot, no retargeted or duplicate write and no spontaneous Resume. |
| N4 | Signed old/new upgrade and retained-bundle rollback with active clients, detached owners, stopped profiles and compatible/incompatible builds. | Previous owner settles before replacement; data, trust and API/TV credentials survive; UI-off access works; interrupted outcomes remain honest. |
| N6 | Signed credential fixtures: normal, denied, locked and interaction-required access, including upgrade/reconnect. | Zero routine prompts, visible blocked states and no unattended retry/dialog loop; explicit consent/unlock is recorded separately. |
| N8 | Authenticated installed Claude Code and signed isolated corpus harness: all 15 tools and nine M6 workflows, search on/off and all-tools-upfront fallback. | Real visible definitions/results, calls and turns meet budgets; confirmation, guards, unknown outcomes, slow private lookup, auth refresh/revocation and connection repair remain correct. Endpoint verification alone is insufficient. |

N8 requires measured client/model usage and captured visible definitions,
including both result channels, duplication and cache/runtime conditions.
The byte/4 proxy cannot substitute for actual token measurements. Current gates
are catalog/instructions ≤1,900 tokens, ordinary receipt ≤180, read summary
≤300, exceptional fingerprint/cleanup ≤450, and workflow ≤1,700 with search or
≤2,800 upfront, subject to the [M6 workflow rules](milestone-06-mcp-budget.md).
If measured usage exceeds a gate, trim and remeasure or obtain an explicit
owner re-baseline. The budget decision remains open.

## Human and physical-device acceptance

- [ ] **M9 signed visual parity.** Finish Wake behavior and visual review of
  setup, saved-TV restore, selection/configuration conflicts, dirty drafts,
  confirmations and blocked/recovery states on the final signed GUI. Existing
  same-Mac Pair/re-pair and reconnect evidence is historical and limited to
  its artifact; see the [M9 audit](milestone-09-acceptance-audit.md).
- [ ] **N9 keyboard review.** Traverse Power, Remote, Settings, setup,
  selection, pairing, Wake/cancel, Forget and server/MCP controls with keyboard
  input. Check visible focus, traversal, activation, Escape, cancellation and
  focus restoration across empty, busy, blocked and recovery states.
- [ ] **N9 VoiceOver review.** Schedule a session and verify accessible names,
  roles, states and announcements for status/outcome changes. This remains
  blocked until observed; screenshots and fake-client tests cannot close it.
- [ ] **N10 physical target-TV matrix.** With the owner present, test
  discovery/manual probe, fingerprint confirmation, Pair/re-pair, selection,
  connect, representative remote keys, network loss, Wake/cancel and Forget.
  Record actual panel/audio/player/power responses separately from socket
  writes, Wake packets or readiness. Restore the agreed network, pairing,
  selection and final power state.
- [ ] **N11 hands-on removal/recovery.** Obtain session-specific consent,
  retain a working signed bundle, then follow Disconnect → Stop → Trash →
  reinstall with running and already stopped owners and a controlled config
  conflict. Verify durable disable/Stop, settled owner, retained TV data and
  no restored access from reinstall alone. End with explicit Resume →
  Connect → Verify and separately observe actual Claude attachment.

## Waived rows

These are **waived**, not passed, under the recorded
[owner decision](phase-2.md#owner-decisions). Do not reopen them as mandatory
work without a scope change; retain their substitutes and residual risks.

| Row | Substitute evidence | Residual risk |
| --- | --- | --- |
| N5: second account / fast user switching | Deterministic cross-UID rejection and per-user runtime-directory checks. | Real session switching remains unobserved. |
| N7: separate-host Local Network attribution and deny/revoke/restore | Same-Mac protocol fixture evidence plus N10 observed TV traffic when available. | Native process attribution and denial/revoke recovery remain unproven. |
| N8 subrow: disposable-account production helper smoke | Required signed isolated corpus harness with isolated `CLAUDE_CONFIG_DIR`, plus development helper tests. | Fresh-account production identity/Keychain behavior remains unobserved. The signed corpus/model lane is still required and pending. |

## Final phase acceptance

- [ ] Build and identify the final signed Version v2.0.0 artifact. Record
  source revision/diff, executable SHA-256, signing class/TeamIdentifier,
  macOS build and client/runtime versions with each native result.
- [ ] Reconcile M6's final acceptance with the revised credential scope;
  legacy migration experiments are removed from scope. Complete M9/M10
  acceptance and M11's requirement-by-requirement review without transferring
  or silently waiving their remaining gates.
- [ ] Run `cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings` and the full `cargo test` on the
  final source/configuration. Investigate failures and retain earlier failed
  evidence even when a later run passes.
- [ ] Link sanitized results to the implementation ledger and acceptance
  reviews; update architecture, UX, installation and troubleshooting guidance
  for any fixes. Exclude credentials, private input and local-network details.
- [ ] Mark Phase 2 Done only when each required checklist item has accepted
  evidence or an applicable recorded waiver. Phase 3 implementation remains
  outside this checklist.
