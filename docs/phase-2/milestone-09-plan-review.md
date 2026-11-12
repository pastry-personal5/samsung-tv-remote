# P2-M9 plan review

Date: 2026-11-12. Status: Plan reviewed; implementation remains Planned.

Reviewed the overview and architecture against M8's completion/follow-up
review, actual native client and GUI probe, executable dispatch, Keychain
adapter, bundle builder and M6's revised acceptance scope. This record is a
planning review, not implementation acceptance. Existing working-tree code
and unrelated planning changes were preserved.

## Owner interview

The owner confirmed during this review:

- Automatically Connect only when a saved TV is disconnected and no
  connection, Pairing, Wake or recovery is active. Never repeat after
  reattachment. The architecture consumes a single launch opportunity,
  including a permanent skip for a ready/busy/empty restored state.
- Stop requires confirmation that it affects every client. Keep the GUI open
  with Resume afterward.
- Document the real-TV smoke procedure carefully, but do not make its
  execution an M9 exit gate. The broader hardware matrix remains M11.

Existing same-executable/on-demand launch, sticky Stop, explicit incompatible
restart, fresh isolated profile, no legacy migration, bounded reconnect and
no mutation replay decisions remain in force. No product question remains
open from this interview. Technical choices below need implementation
evidence, not another product approval.

## Findings and corrections

| Priority | Finding | Plan correction |
| --- | --- | --- |
| P1 | Architecture calls M8 partial wire work even though its accepted handoff now includes HTTP, process lifecycle and a GUI probe. | Name the completion, wire, transcript and implementation artifacts; reuse their modules, budgets and reliability regressions. Keep signed production evidence distinct. |
| P1 | Probe projections reuse application snapshots and raw JSON outcomes; errors lose admission detail and presentation has two completion paths. | Require a fakeable client port, safe typed DTOs/errors and one exhaustive result path. Preserve every workflow's uncertainty and cleanup facts. |
| P1 | Shared SSE events do not deliver private terminal outcomes, and the current client omits initial retained references. | Decode atomic private references, look up bounded outcomes and merge monotonically. Recovered history cannot infer a local intent or run a follow-up mutation. |
| P1 | Unconditional GUI-launch Connect can interrupt an active server session; target/generation checks cannot prevent a same-generation race. | Owner selected idle/disconnected-only launch Connect, once per GUI lifetime. Add an atomic owner-side idle guard through a narrow optional request flag; preserve explicit Connect behavior and test competing clients. |
| P1 | Reusing the legacy Keychain adapter would open the wrong service and obscure locked/denied states. | Freeze disjoint production/development/legacy namespace identities; add noninteractive typed credential ports and test missing versus blocked access. |
| P1 | Signed feasibility occurs late, and a documented signed blocker could be mistaken for M9 completion. | Prototype identity/fresh credentials first with disposable items. Signed production gates block cutover and Done. Same executable cannot enforce mode isolation or human authorization. |
| P2 | Multiple pending maps/lookups can duplicate recovery or reopen completed UI work. | One 64-intent client tracker/scheduler, concurrent SSE, bounded completed deduplication and cancel-to-uncertain semantics. Remove GUI polling. |
| P2 | Ignoring all duplicate snapshots conflicts with M8's strict event ordering and can suppress readiness on same-revision reattach. | Distinguish validated UI callbacks from wire events. Install fresh attachment/private context even at the same revision. |
| P2 | Selection/save/Connect chains and draft edits during pending saves are underspecified. | Inventory actual messages/results and require originating intent, captured terminal preconditions and a compatible current snapshot for each continuation. |
| P2 | Stop impact, lost Stop reply and credential recovery lack complete user behavior. | Confirm all-client impact; distinguish durability/settlement; keep Resume visible. Explicit credential recheck never regrants revoked access or retries mutations. |
| P2 | Boundary checks could miss aliases or reject legitimate pure shared values and mode dispatch. | Specify forbidden dependencies, a narrow allowed dispatcher and negative fixtures, covering messages/tasks and production/fake composition. |
| P2 | Hardware smoke evidence and signed release gates are ambiguous. | Add an optional real-TV runbook with explicit observation limits and cleanup; retain signed macOS gates and M11's broader hardware matrix. |

## Technical decisions to record during implementation

- Exact safe DTO module placement, typed error mapping and port shape. Use
  existing wire fixtures; do not grow a second application service in the UI.
- Completed-message/dedup bounds and bounded lookup scheduling within M8's
  existing quotas. Delayed lookup must never prevent SSE consumption.
- Production namespace manifest, stable Keychain identity and noninteractive
  error behavior; exact peer code requirement and race-resistant verification
  of the connected process. Wrong peers/replay must fail on the real platform.
- A reproducible signed fixture/runbook, required dependency check and cutover
  evidence manifest recording revision plus working-tree fingerprint, bundle
  identity, host, commands, prompt counts and sanitized pass/fail/deferred facts.

## Planning validation

This changes documentation only. Validate changed-document whitespace, local
links and consistency through the repository's delegated validation workflow.
No Cargo, signed-platform or TV execution is claimed by this review. The
implementation must run the contribution guide's required Cargo checks and
produce its own completion review before cutover acceptance.
