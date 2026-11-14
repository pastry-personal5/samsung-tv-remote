# P2-M9 GUI migration review

Status: Draft. The GUI migration and the evidence below are implemented;
original M9 platform acceptance remains open pending the owner's scope decision.
This document does not authorize release or claim unrun native checks passed.

## Delivered behavior

The default Iced GUI uses the typed Control API client for setup, selection,
configuration, remote actions, Wake, cancellation, Forget and recovery. It owns
windows, drafts and captured intents. The server owns TV/network/storage policy
and all retained outcomes. The temporary in-process adapter is removed, and a
required negative-corpus check enforces the GUI boundary.

The signed same executable starts its production server on demand, keeps it
alive after GUI exit, and restores a fresh authenticated snapshot on reopen.
Stop is sticky; Resume and incompatible-build restart are explicit. Automatic
recovery performs lookup without replaying TV effects. A server attachment does
not imply TV readiness.

The actual Apple-signed application's explicit foreground API access mode fixes
the observed standalone-helper approval gap. It verifies signing before any
Keychain call, reads only existing fresh-service GUI/MCP API items and never
prints or changes their values. Inspection makes no Keychain calls. Routine
startup and credential recheck never invoke the foreground path.

## Evidence mapping

The [acceptance audit](milestone-09-acceptance-audit.md) preserves the original
twelve checklist items, detailed test names, native observations and remaining
gaps. The [implementation ledger](milestone-09-implementation.md) records the
predecessor handoff, namespaces and signing experiments.

| Area | Evidence |
| --- | --- |
| Complete client workflows | Fake Iced tasks capture exact epoch/target/generation/configuration preconditions for control and setup. HTTP tests cover every setup, remote, Wake/cancel, cleanup and recovery outcome. |
| Ordering and bounded recovery | Receipt/event order tests, stale-context and epoch tests, retained private-history tests, cancellation/lost-admission recovery, and one transport-owned lookup scheduler. Restored history cannot start local continuations. |
| Draft and selection safety | External selection/configuration changes stop continuations without retargeting drafts; newer edits survive pending saves; explicit discard/reload restores current settings. |
| Cross-client and process behavior | Two-client idle-Connect races, selection and retained-outcome tests; subprocess outage/reattachment tests; fake-clock single-timer/backoff tests. No mutation replay. |
| Lifecycle | Fake-client Stop epoch/durability/settlement tests and subprocess sticky Stop/Resume tests. Signed GUI exit/reopen preserves the same server epoch. The [signed Stop warning](evidence/m9-signed-stop-confirmation.png) was reviewed and cancelled without stopping the server. |
| Signing and credentials | Native signed peer checks and wrong-peer rejection; Apple changed-build disposable CRUD; explicit actual-app approval followed by routine existing-role API access, authenticated incompatible-build rejection and restart. No credential reset or namespace replacement. |
| Real-adapter synthetic workflows | Signed same-Mac TLS fixture Check/Pair/re-pair, one remote receipt, saved-profile restart restore and earlier-Apple-build credential restore. [Restored readiness](evidence/m9-signed-fixture-restored.png) is distinct from [empty server attachment](evidence/m9-signed-connected-empty.png). |
| Cleanup | Approved GUI Forget reports credential/trust/record removal and an empty profile in the [cleanup screenshot](evidence/m9-signed-fixture-cleanup.png). The fixture was stopped; API credentials remain intact. |
| Boundary and data isolation | Mandatory GUI dependency check, no `in_process` adapter, server-only production composition, rejected cross-namespace records, fresh `control-v1` profile and preserved legacy data. |
| Validation | 328 package tests, formatting, all-target Clippy and example compilation/tests pass. Reader tests moved into the package suite. Fixture self-checks pass eight tests. Local documentation links and whitespace checks pass. |

## Unfinished platform acceptance

- Controlled separate-host Local Network attribution, denial/revocation and
  recovery; same-Mac fixture traffic does not prove these OS behaviors.
  Waived on 2026-11-14 by [owner decision](phase-2.md#owner-decisions); it is
  reported `waived`, not passed.
- Signed native Wake/readiness/cancellation and the remaining visual parity
  cases, including draft conflicts and exact terminal-outcome review.
- Native locked and interaction-required credential states and recovery using
  an isolated procedure that does not lock the owner's whole login keychain.
- Final committed release artifact, final signing/peer review and complete
  release/upgrade evidence. Current native follow-up used a preserved candidate
  built from the validated working tree.

The local self-signed cross-build credential failure remains a failed result.
The successful Apple path does not certify all signers or public distribution.
No physical-TV behavior, panel power, accessibility acceptance or MCP delivery
is claimed. The real-TV smoke runbook is supplementary; M10 owns MCP and M11
owns integrated release, accessibility and hardware review.
