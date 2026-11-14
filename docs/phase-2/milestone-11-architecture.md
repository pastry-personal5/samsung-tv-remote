# P2-M11: Integration and Refactor Completion

Status: Active — deterministic integration implemented; native acceptance open.

## Baseline and ownership

The current implementation is one Rust package and one signed executable with
early server/management dispatch. `server/process.rs` composes production
adapters; `application` owns TV policy and effects. `control_client` owns GUI
HTTP/SSE and bounded recovery. `presentation/iced` owns local views, drafts and
captured intents. MCP enters the same `server/control_http` listener and
application submission path. M11 preserves these boundaries.

M9 already removed `InProcess` and added the mandatory
`scripts/check_gui_boundary.py` negative corpus. The installer already stages
and verifies a bundle, settles exact-path processes, publishes by rename,
retains the previous bundle and attempts rollback. M10 implements shared MCP
transport, enrollment, header helper, the configuration coordinator, GUI
Check/Connect/Verify/Disconnect, relocation repair and conservative 2.1.325
preflight. Its signed helper/client, installed and model-visible acceptance
remains open. These are baseline facts, not M11 acceptance.

| Responsibility | Existing code and verification seams |
| --- | --- |
| Shared admission, operations, storage fences | `src/application/{control_service,operations,runtime}.rs`; `tests/headless_runtime.rs`, `runtime_limits.rs`, `forget_recovery.rs`, `support/storage_faults.rs`. |
| Authenticated transport and MCP | `src/server/control_http/`, `src/server/mcp/`; `tests/control_http.rs`, `mcp_protocol.rs`, `support/mcp_http.rs`. |
| Client projection and reconnect | `src/control_client/`, `src/presentation/iced/`; `tests/control_client.rs`, `control_client_rotation.rs`, Iced `client_tests.rs`. |
| Ownership, launch, Stop and credentials | `src/server/{lifecycle,bootstrap,access}/`, production composition and macOS adapters; `tests/server_process.rs`, `server_lifecycle.rs`, `server_bootstrap.rs`, `server_access.rs`, `mcp_helper.rs`, `mcp_connection.rs`. |
| Installation and rollback | `scripts/install_macos_bundle.py`, `scripts/build_macos_bundle.sh`; `tests/bundle_install.rs`, `bundle_signing.rs`, `test_macos_bundle_install.py`. |
| GUI dependency boundary | `scripts/check_gui_boundary.py`, `tests/gui_boundary.rs`. |

Use existing tests where they establish the required behavior. Add combined
transport tests where independent adapter tests cannot establish cross-client
ordering. A new `tests/control_integration.rs` may own those scenarios, sharing
fake ports with existing fixtures. Keep timing/order assertions deterministic
with barriers, fake clocks and effect counters; arbitrary sleeps are not proof
that a race is absent. Do not add a second runtime, queue, service policy or
protocol adapter solely for acceptance.

## Entry freeze and reliability

Slice 1 inventories acceptance rows against named existing tests and identifies
missing interleavings before implementing changes. Record source revision,
bundle identity/version/hash/TeamIdentifier, macOS version, installed Claude
Code/runtime revision, protocol contract, schema/storage version and fixture
namespace. Freeze actual command/result retention, deadlines and resource
limits from source and the M10 contract; do not copy historic values as if they
were verified on the candidate.

Keep M9 and M10's original acceptance owners. Deterministic M11 fixture work may
proceed while native evidence is blocked. Installed end-to-end checks can use
the implemented M10 connection actions against an isolated
`CLAUDE_CONFIG_DIR`. They never use the owner's configuration. The two
intermittent failures in the [M10 review](milestone-10-code-review.md#validation)
and [ledger](milestone-10-implementation.md#verification-record) need diagnostic
reproduction: concurrent helper starts and release of the final enrollment
ownership guard. Capture secret-free process/lock lifetime facts, isolate the
interleaving, then add a regression for the identified cause. A passing rerun
alone does not close either issue.

## Cross-client state and operations

Run the typed GUI client and real MCP route against one listener and fake TV
ports. They have distinct GUI/MCP principals; multiple MCP sessions share the
MCP identity. Compare equivalent state facts from one epoch/revision, allowing
surface-specific summaries. Never compare untagged snapshots taken at different
moments as if they were atomic.

- Race subscription attachment with completion; the observer receives either
  the captured snapshot or its subsequent ordered event, without losing the
  outcome. Overflow or a stream gap requires a fresh snapshot and bounded
  private-result recovery; an observer never blocks the state owner.
- Race GUI selection/configuration changes with MCP commands and vice versa.
  Captured target/generation/revision guards must reject stale intent without
  retargeting drafts, queued commands or continuations. A new epoch rejects old
  handles and mutations even if the saved device ID is unchanged.
- Send equivalent typed commands via both routes under the same authorized
  principal/key when testing shared deduplication. Across GUI/MCP principals,
  test lookup isolation and independent key ownership; equal key text alone
  must not grant cross-role receipt access. Multiple MCP sessions must share
  the documented identity, quotas and retained outcomes.
- Disconnect before admission, after admission, during a write, and after
  terminalization but before reply. Use original-key lookup for lost admission
  receipts and operation lookup for known handles. In-window duplicates return
  the retained result without another effect; changed payload conflicts.
  Expiry/capacity behavior follows the frozen registry contract. Crash-lost or
  expired lookup is unknown, never evidence that nothing was sent.
- Saturate mixed `/v1`, `/mcp`, SSE, parsing and lookup-wait paths using the
  [M10 capacity contract](milestone-10-contract.md#shared-capacity-and-deadlines).
  Prove aggregate and per-principal bounds, recovery/management access,
  cancellation cleanup and no permit leak after malformed or stalled requests.
- Inject Pairing/journal, credential, configuration and Forget partial failures.
  Preserve durable recovery markers and version fences. Client disconnect or
  cancellation of a lookup does not cancel admitted effects or manufacture
  successful persistence. Record exact writes/packets and recovery commands.

No automatic mutation retry is introduced. A Wake send, remote socket write,
usable remote connection and observed panel response remain different facts.

## Signed lifecycle, installation and rollback

Retain the per-user protected endpoint/ownership/authentication design. Verify
owner UID, signed peer and readiness before attaching; port occupancy is not
identity. Concurrent GUI/helper starts converge on one owner. A lock remains
held through admitted management, worker drain and listener closure. A second
account cannot consume the first account's endpoint or bearer. The native
second-account and fast-user-switching check is waived by the owner decision.
Its substitute is deterministic cross-UID peer/owner rejection in
`server_access`/`server_bootstrap` plus per-user runtime-directory tests. The
residual risk is that real session switching is not observed.

Exercise graceful Stop separately from process crash/termination. Stop persists
before acknowledgment; marker-observed after a lost reply does not prove drain
finished. Resume is explicit. Crash permits on-demand restart with a new epoch;
recovery starts no TV action by itself and does not replay pending effects.
Sleep/wake invalidates stale connections and causes bounded resynchronization,
without an implicit Resume. GUI close leaves an attached MCP client available.
No launch-at-login registration is added.

Use the existing installer rather than replacing it. Test source/stage signature
failure, simultaneous installers, active-process identity/PID reuse, settlement
timeout, concurrent relaunch, rename failure, post-publication verification and
rollback failure. A process launched during replacement must cause safe abort
or participate in a serialized handoff; the check-then-rename interval needs
explicit fault injection. M11 now takes process-lifetime shared executable
locks before bundle mode dispatch and exclusive old/staged executable locks
through publication and rollback. A cooperating relaunch after settlement
aborts safely; a launch during publication fails before opening a profile.
Post-quarantine process checks also detect late legacy launches before
publication. The [implementation ledger](milestone-11-implementation.md)
records regressions and the remaining signed legacy-upgrade gate. Retain both
artifacts when rollback fails and print recovery
paths without profile data or credentials.

Rollback reuses the installer. Run `install` with the retained
`.samsung-tv-remote.previous-*.noindex` bundle as the source, so it verifies
the signature and settles running processes exactly like an upgrade. Each
successful install currently leaves one more retained bundle. Document manual
cleanup of older retained bundles, keeping the newest. Do not add automatic
pruning unless evidence shows the accumulation is a problem.

The installer must preserve sticky Stop, disabled enrollment and all data. The
signed upgrade matrix covers an older compatible owner, an incompatible owner,
a detached owner, GUI+MCP traffic and an intentionally stopped profile. Record
interrupted work as retained or unknown, never replay it after replacement.
Test rollback to the retained bundle against the resulting profile. If a schema
change is necessary, back up before conversion, publish atomically and prove a
compatible restoration procedure; otherwise keep formats unchanged. Copying a
bundle alone is not a data rollback or Keychain-access guarantee.

## Removal guidance

Owner decision (2026-11-14): removal adds no new action, fence or state. The
user guidance is:

1. **Disconnect Claude Code** in Settings → Servers. It durably disables
   enrollment and revokes live MCP access, then removes only the unchanged
   managed entry. A changed entry is a conflict that keeps revocation.
2. **Stop** the server. It persists before acknowledgment and needs explicit
   Resume.
3. Move the application bundle to Trash. TV records, trust, Keychain items,
   Stop and disabled enrollment remain. Full erasure is out of scope.

The existing mechanisms are expected to provide the required guarantees;
slice 3 confirms each one before the guidance ships. A disabled
enrollment never resurrects a previous bearer. A helper from a relocated or
reinstalled bundle is denied until an explicit Connect issues a new grant.
Relocation repair is itself an explicit Connect. Stop is never cleared by
Verify or a helper.

Slice 3 maps this sequence to existing tests and adds only missing cases. The
existing tests include `stopped_profile_management_is_durable_…`,
`disconnect_retains_revocation_when_removal_conflicts`,
`malformed_client_file_does_not_hide_revocation_and_failed_removal_stays_disabled`
and `relocation_is_repairable_only_at_the_saved_trusted_endpoint`. Verify,
then document, each of these cases:

- Disconnect while the server is already stopped uses offline management, and
  neither starts a TV runtime nor clears Stop. If it cannot, the guidance
  must give the working order.
- Disconnect followed by a crash at each step, a lost reply, a delayed old
  helper or a concurrent Connect leaves access disabled or explicitly unknown.
- A leftover entry after a failed removal plus a reinstall or relocation
  offers only the explicit repair/Connect path, never silent access.
- Retry with an absent or externally changed entry is safe and visible.

Automated tests use isolated profiles and `CLAUDE_CONFIG_DIR`. Automation never
stops the owner's installed server or deletes owner records. Signed bundles
always use the production profile, so the native N11 check runs on the owner's
installation. It runs hands-on, with consent given at that session, and follows
a runbook that ends with Resume, Connect and Verify.

## Evidence topology and acceptance

Use the [acceptance matrix](milestone-11-acceptance.md). Deterministic tests use
fake network/pairing/storage ports and isolated development namespaces. The
signed isolated corpus harness measures real client behavior with synthetic
effects. Disposable-account production smoke verifies actual identity/helper
and credential paths. Separate-host traffic establishes native Local Network
attribution/denial/recovery. Physical target-TV checks establish only the
behaviors actually observed. No tier substitutes for another. The exceptions
are the rows waived by the [owner decision](phase-2.md#owner-decisions). They
are reported as `waived` with their named substitute and residual risk, never
as passed.

The M6 gates are the starting targets: catalog/instruction 1,900; ordinary
receipt 180; read summary 300; exceptional fingerprint/cleanup 450; complete
workflow 1,700 with search or 2,800 upfront. Today's proxy for the implemented
catalog is 57,345 bytes, or 14,337 byte/4 units, including output schemas and
annotations ([M10 review](milestone-10-review.md#client-and-budget-evidence)).
The 2,136/3,036 figures are historical and describe different proposed
definitions. Neither figure is a model token count.

Slice 1 therefore measures first. Capture what the installed Claude Code
actually sends to the model, with `scripts/measure_mcp_corpus.py` and the
signed isolated harness. Measure the definitions in both tool-search modes,
both result channels, all nine workflows, slow-effect lookups, helper refresh
and model turns. Then stop at an owner decision: if the measurement exceeds a
gate, the owner either trims the catalog or re-baselines, and the decision is
recorded in [phase-2.md](phase-2.md#owner-decisions). Phase 3 tool additions wait
for that decision. Neither path may remove safety guards, confirmations or
honest unknown outcomes. Rerun after any M11 catalog, projection, helper or
client change.

Native review records routine Keychain prompt counts, exceptional consent and
blocked-state recovery separately. Use isolated credential procedures; do not
lock the owner's entire login keychain. The final target is the owner's local
Apple-signed installation; notarization/public distribution remain out of scope.
Accessibility and TV evidence use the same final artifact. Keyboard focus,
VoiceOver names/roles/state and announcement behavior must be observed, not
inferred from screenshots. Redact hostnames, addresses, tokens, private text
and certificate/device identifiers from committed artifacts.

## Completion artifacts and validation

Create `milestone-11-implementation.md` when implementation begins and
`milestone-11-review.md` at completion. The latter maps every overview checkbox
to named tests/native records, failures, repairs and remaining blockers. Store
sanitized evidence under `docs/phase-2/evidence/` with artifact provenance.

Update `docs/architecture.md`, the UX specs, `docs/contribution-guide.md`,
`README.md` and the documentation index to match accepted behavior. Add user
troubleshooting for stopped versus crashed owners, incompatible builds,
locked/denied credentials, unavailable Local Network access, stale/unknown
operations, saved-but-unverified MCP configuration, rollback and removal.
Keep planned functionality out of current-user instructions until it exists.

Run targeted affected suites during changes and the
[contribution guide](../contribution-guide.md)'s full formatting, Clippy and
test gates before a PR.
Recheck the exact source/artifact after relevant repairs; an automated pass
never accepts a missing native, client or hardware row. M11 and Phase 2 remain
incomplete while any required, unwaived gate is failed or blocked.
