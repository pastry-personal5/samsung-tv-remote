# P2-M11: Integration and Refactor Completion

Status: Active — implementation and deterministic integration; native acceptance open.

## Goal

Accept the signed GUI and MCP clients as one per-user control system. Harden
installation, recovery and removal; prove that both clients preserve the same
selection, operation and uncertainty contracts; finish the boundary,
accessibility, hardware and documentation review for the local macOS release.

This is the implementation plan. The [implementation ledger](milestone-11-implementation.md)
and [acceptance review](milestone-11-review.md) record evidence. M9 native acceptance
and M10 delivery remain incomplete. Planning M11 does not waive those gates or
transfer their unfinished implementation into a supposedly completed milestone.

## Entry and predecessor gates

- Use the stable M8 service/HTTP boundary and the implemented M9 client-only GUI.
  The in-process GUI adapter is already removed; M11 audits and strengthens the
  existing boundary rather than scheduling its removal again.
- Track M9's blocked credential states,
  signed Wake/visual review and final artifact evidence through its
  [acceptance audit](milestone-09-acceptance-audit.md) and
  [review](milestone-09-review.md).
- M10 GUI Check/Connect/Verify/Disconnect, relocation repair and conservative
  preflight for Claude Code 2.1.325 are implemented (2026-11-14). Track the
  rest through its [implementation ledger](milestone-10-implementation.md) and
  [review](milestone-10-review.md): enrollment/publication fault coverage, the
  reproduced ownership-lock assertion, signed helper/client evidence and
  real model-visible measurements.
- The owner's [2026-11-14 decision](phase-2.md#owner-decisions) waives the
  second-account, separate-host Local Network and disposable-account
  production-helper rows in M9, M10 and M11. Each waived row records its
  substitute evidence and residual risk. A waived row is never reported as
  passed.
- Deterministic fixture preparation and documentation can proceed against the
  stable interfaces. Signed integrated acceptance requires the corresponding
  M9/M10 gates. Record every dependency as passed, failed or blocked; do not
  mark it passed because an isolated test or a later rerun succeeded.

## Scope and constraints

Keep one same-executable signed GUI/server composition, one server-owned
runtime, one selected TV and shared admission/recovery policy. Extend existing
lifecycle, installer and test seams before introducing new infrastructure.
No login-time launch, public distribution/notarization, device-key import,
new TV capabilities or automatic replay of uncertain effects is in scope.

Preserve legacy records and Keychain items without importing them. Preserve
fresh production data across upgrades. Removal adds no new feature. The user
runs the existing **Disconnect Claude Code**, then **Stop**, then moves the
bundle to Trash. M11 tests and documents that sequence. Durable disable and
sticky Stop are expected to prevent helper resurrection and to keep a
reinstall from restoring access; slice 3 confirms this. Deleting the bundle alone cannot stop a
detached server; there is no Trash hook. Full data/credential erasure is out
of scope.

## Delivery order

| Slice | Deliverable | Exit evidence |
| --- | --- | --- |
| 1. Baseline and dependencies | Requirement-to-test inventory, exact candidate/build/signing/client identifiers, isolated fixture topology, fault schedule and predecessor ledger. Investigate the two M10 intermittent ownership/helper failures. Measure the definitions and results the installed Claude Code actually shows the model. | Every acceptance row has a test owner, an evidence tier and a `waived` mark where one applies. Unresolved reliability failures remain explicit blockers. A measured catalog over 1,900 stops at an owner budget decision. |
| 2. Cross-client correctness | Deterministic GUI-client plus MCP tests through the same listener/runtime, including storage and transport faults. | Exact admission/effect counts, consistent tagged state, private lookup and no replay across each fault in the acceptance matrix. |
| 3. Lifecycle and removal | Installer hardening where evidence finds gaps; tested Disconnect → Stop → Trash removal guidance; rollback by reinstalling the retained bundle. | Fake process/file/credential tests cover races, partial completion and retry; retained data and Stop/enrollment guards survive restart and reinstall. |
| 4. Signed integration | Final Apple-signed candidate, installed Claude Code workflows, upgrade/rollback, crash/sleep/session isolation and prompt accounting. | Native evidence from the exact artifact, with M9/M10 dependencies resolved and no routine prompts. |
| 5. Accessibility and hardware | Hands-on keyboard-only review and target-TV setup/control/Wake/recovery, with honest effect versus observation reporting. Prepare the VoiceOver runbook; that session is not yet scheduled. | Sanitized reproducible evidence; defects repaired and relevant checks rerun. VoiceOver stays `blocked` until it runs. |
| 6. Completion review | Current user/developer/architecture/UX/troubleshooting docs and requirement-by-requirement release review. | Required validation passes and every completion item has accepted evidence. |

See the [architecture](milestone-11-architecture.md) for ownership and failure
contracts and the [acceptance matrix](milestone-11-acceptance.md) for scenarios.
The [native runbook](milestone-11-native-runbook.md) prepares signed/client,
accessibility, hardware and hands-on removal sessions.

## Completion checklist

- [ ] Predecessor ledger resolves all dependent M9/M10 gates with linked evidence.
  Intermittent helper cold-start and enrollment ownership failures have an
  explained resolution and regression evidence, not only successful reruns.
- [ ] Shared GUI/MCP tests prove atomic subscription, ordered tagged state,
  bounded slow-observer recovery, stale selection/configuration/epoch rejection,
  Pairing conflicts and shared queue pressure. Loss of a reply, observer or
  client never replays a TV effect or cancels already admitted work implicitly.
- [ ] Operation recovery respects principal privacy and the documented retention
  window. Same-key duplicates within that window cannot send twice; expired
  or crash-lost outcomes are unknown. New intent after uncertainty requires
  a fresh explicit decision and current guards.
- [ ] Signed cold launch, simultaneous start, GUI close/reopen with MCP active,
  Stop/Resume, incompatible restart, crash, sleep/wake, upgrade, rollback and
  the removal sequence pass. Unrelated listeners and cross-user peers
  fail closed; neither client clears intentional Stop. No login launch is added.
- [ ] The documented removal sequence (Disconnect, Stop, Trash) works with a
  running or already stopped owner. It revokes MCP before exact-entry removal,
  settles the owner, preserves data and reports partial failures. Delayed
  helpers, stale management intents, a relocated or reinstalled bundle and a
  leftover entry cannot reopen access without an explicit Connect. Retry stays
  safe when the entry is absent or was changed elsewhere.
- [ ] Legacy and fresh records/trust/Keychain items survive upgrade and rollback;
  any necessary schema change has reversible backup/recovery tests. Development
  fixtures never use production profile overrides or owner credentials.
- [x] Mandatory GUI boundary and fake-client command/projection tests pass.
  Views perform no HTTP calls; presentation owns no TV/storage/service policy.
  Retained server-side coordinator code is judged by ownership and callers,
  not removed merely because its name appears in historical architecture.
- [ ] Installed Claude Code runs all nine M6 corpus workflows in both tool-search
  modes, with all-tool parity and recovery cases. Real model-visible definitions,
  results, duplicated channels, calls and turns are measured. They meet the M6
  budgets, or the owner's recorded decision to trim or re-baseline. Until that
  decision exists, an overrun is a failed gate. Re-baselining cannot remove
  safety guards.
- [ ] Signed routine start/reopen/reconnect/upgrade produces zero routine Keychain
  prompts. Denied, locked and interaction-required states fail visibly with no
  unattended prompt loop. Gatekeeper, Local Network, legacy ACL/unlock, Pairing
  and optional client consent are counted separately. Separate-host Local
  Network attribution is waived. Its substitute evidence and residual risk are
  recorded per the owner decision.
- [ ] Keyboard-only review covers all primary flows, confirmations,
  busy/error/recovery states and focus restoration. VoiceOver review covers
  names, roles, state and announcements; it stays `blocked` until a session is
  scheduled. Sanitized target-TV evidence
  covers setup, selection, remote actions, reconnect, Wake/cancel and Forget;
  packet/socket-write facts remain distinct from observed TV behavior.
- [ ] Final artifact and source revision are recorded; `cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test` pass. Current docs
  and `milestone-11-review.md` link every requirement to evidence. A row covered
  by the owner decision is reported `waived`, with its substitute evidence.
  Every other unavailable native, client or hardware check stays blocked, and
  M11 stays incomplete.

## References

- [Phase plan](phase-2.md)
- [M6 acceptance and test ownership](milestone-06-acceptance.md)
- [M6 model-visible budgets](milestone-06-mcp-budget.md)
- [Target architecture](target-architecture.md)
- [Lifecycle and security research](server-lifecycle-security.md)
- [Control API Server internals](control-server-internals.md)
