# P2-M10 implementation ledger

Status: Active; delivery and installed acceptance incomplete.

The owner's instruction to complete M10 authorizes implementation against the
stable service boundary while M9 native acceptance remains open. The
[phase-plan exception](phase-2.md) does not waive dependent production gates.
The [overview checklist](milestone-10-overview.md#completion-checklist) remains
the completion authority; this ledger records progress toward the whole goal.

## Implemented foundation

- `/mcp` enters the existing authenticated `HttpAdapter` and listener. Unique
  JSON decoding, per-request metadata/header/version checks, JSON-RPC framing,
  revision discovery and the deterministic 15-tool catalog are implemented.
- All 12 mutations use the existing strict `/v1` decoder and shared authorized
  service submission. Private lookup supports original-key recovery, bounded
  waiting and read cancellation independent of admitted effects. Receipts keep
  their original private key, retained outcomes and terminal guards.
- Saved-TV pages, selected context, startup/admission/recovery summaries and
  atomic inspection include orphan cleanup markers. Authenticated cursors bind
  principal/epoch/query and relevant saved configuration, without a page registry.
- The shared 64/16 request allowance now includes four/two classification slots
  carved from ordinary capacity. POST reads use query slots; waits consume
  bounded subsets and existing owner subscriber capacity. `/v1` and SSE remain
  on the same transport.
- Production and development composition now distinguish durable MCP enrollment
  from readable API Keychain items. GUI-only management uses expected generation
  and an operation key; helpers cannot implicitly enable it. Disable fences the
  live lease before persistence, including a failed-write state that bootstrap
  refresh cannot undo. A new explicit grant derives a fresh bearer while retaining
  the same principal. Blocking management retains profile ownership after its
  HTTP task closes. Live/offline controller methods are implemented; the offline
  executable mode and configuration workflow remain tracked below.
- The early signed `--mcp-headers` mode has an eight-second total deadline,
  validates the pinned destination and enrollment generation around protected
  MCP-role startup/attachment, and emits one header object only on success.
  Missing enrollment, changed/external destinations and intentional Stop return
  no credential. Offline management now prepares a stable endpoint or changes
  enrollment under profile ownership without constructing a TV runtime.
- Resumed the interrupted user-configuration slice. The narrow writer preserves
  unrelated settings, compares exact entries for relocation/removal, uses the
  client's sibling-directory lock and retains displaced files for recovery when
  a noncooperating writer races publication. A separate process-held advisory
  lock serializes whole app Connect/Disconnect workflows and remains held by a
  blocking edit after cancellation. The coordinator saves configuration before
  grant, durably disables before removal and reports configuration/enrollment
  progress separately, including unknown outcomes. Identical Connect preserves
  its enrollment; stale generations cannot silently regrant after Disconnect.
  Removal is safely retryable when the expected entry is already absent.

The [contract record](milestone-10-contract.md) fixes mappings, limits, response
classes and projection choices. On 2026-11-14 the coordinator was wired through
the typed native-client port to explicit GUI Check/Connect/Verify/Disconnect
and confirmed relocation repair. Partial failures retain the original management
intent for explicit retry. Preflight checks the supported client version,
known local/project conflicts and restrictive/managed files; isolated Claude
Code 2.1.325 tests establish writer/header-entry and directory-lock compatibility.
The development probe requires an explicitly isolated Claude configuration.
App-side authenticated catalog verification remains distinct from actual Claude
attachment. See [connection guidance](milestone-10-connection.md) for limitations.

Discovery/probe now inspect their admitted private operation once before
projection, returning an already available terminal observation without any
subscriber, wait allocation or resubmission. Slow effects still return pending
receipts immediately. Corpus inputs are checked against all 15 shipped schemas;
certificate-change re-pair coverage keeps device identity and rejects a stale
fingerprint confirmation before Pairing.

## Implementation review

The [2026-11-13 code review](milestone-10-code-review.md) repairs original-intent
Connect recovery, unambiguous IDs on duplicate-member RPC rejection and bounded
inspection labels. The 2026-11-14 follow-up also preserves a second writer during configuration
rollback and binds enrollment retry keys to the original expected generation.
Signed/native/client acceptance remains open.

## Verification record

- Early delegated `cargo fmt --all` and `cargo check --all-targets` passed.
- Claude Code 2.1.322 and the supported user-scope JSON writer were inspected
  using fresh isolated `CLAUDE_CONFIG_DIR` directories. No owner entry was
  written and no installed endpoint/client session is accepted from that check.
- The transport foundation passed 53 focused tests, Clippy and the full Cargo
  suite, including heap/process/runtime-limit checks. Actual tool results are
  validated independently against output schemas using the dev-only offline
  `jsonschema` validator. The first run found a test expectation inconsistent
  with the existing ready-TV Wake policy; the test preserves that shared
  rejection and separately exercises pending Wake/cancellation after one send.
- Enrollment and live management then passed all required Cargo checks with
  165 unit tests and 15 subprocess cases, including initial denial, regrant
  private-result continuity and durable disable across restart. A restricted
  sandbox run failed loopback binds; the same default-concurrency tests passed
  with the documented isolated loopback permission. No application workaround
  or test serialization was needed.
- Header-mode parser and isolated subprocess tests passed, as did Clippy.
  The resumed 2026-11-13 delegated validation passed `cargo fmt --all`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test` at default
  concurrency: 381 tests passed, including 186 unit tests, the new offline
  connection workflow and all 18 server-process cases. An earlier full run
  failed one concurrent cold-start helper exit assertion; its focused rerun
  passed. After adding credential-safe status/stderr diagnostics, the full run
  also passed. The earlier failure has no identified root cause; retain it as
  a helper reliability follow-up rather than treating a rerun as a fix.
  These are development-namespace tests, not production signature/Keychain,
  installed-client lock interoperability or native-permission evidence.

The implementation review's final delegated run passed formatting, Clippy and
all 384 tests at default concurrency. Its first run hit a separate intermittent
ownership-lock assertion at `enrollment.rs` immediately after dropping the last
guard; focused and subsequent full runs passed. No root cause is established.
See the [review verification record](milestone-10-code-review.md#validation).

The 2026-11-14 delegated full run passed 399 tests (195 unit and 18
server-process cases included), with captured zero exit statuses. Two native
regressions added afterward passed focused execution; latest-tree formatting,
Clippy and whitespace checks also passed. The initial Settings panel was rendered and visually reviewed
in an isolated development profile. Claude Code 2.1.325 writer/header-entry and
directory-lock checks passed in disposable configuration. Actual CLI `mcp get`
and `mcp list` then reported Connected through the destination-bound dynamic
helper against the actual development server, cold-started with the GUI closed
and stopped cleanly afterward. This does not establish signed production
identity or model tool execution. A fresh isolated
Claude auth check has no logged-in model session, so actual-model budgets are
still unavailable. The earlier reproduced ownership-lock failure remains an
unresolved reliability follow-up despite the final passing suite. See the
[acceptance review](milestone-10-review.md) for the full scope and remaining gates.

## Work remaining

1. Finish the acceptance coverage audit and reconcile actual client-visible
   result channels/workflow costs with the historical corpus projections.
2. Finish enrollment crash/race coverage and configuration publication fault
   coverage; verify protected helper fencing across subprocesses and the
   actual-client refresh/rotation behavior. Installed 2.1.325 directory-lock
   interoperability now has isolated evidence.
3. Complete signed-helper cold-start, termination/deadline, hostile-peer and
   credential-rotation client evidence, including the real production identity.
4. Review the new native connection controls visually and in signed production;
   effective organization policy and future project overrides are not inferred
   from the conservative file preflight.
5. Execute the nine actual-client corpus workflows with tool search on/off and
   all-tool recovery parity in the signed isolated harness. Record real visible
   definitions/results, calls/tokens and justified budget exceptions.
6. Complete production-helper/client smoke in a disposable macOS account,
   M9-dependent signing/credential gates and separate-host native attribution
   evidence. Same-Mac synthetic success is insufficient for Local Network consent.
7. Resolve the reproduced intermittent ownership-lock release failure and finish
   the requirement-by-requirement [acceptance review](milestone-10-review.md).
   The 2026-11-14 [owner decision](phase-2.md#owner-decisions) later waived the
   disposable-account production-helper and separate-host lanes. They are
   reported `waived` with substitutes, not passed.
   Current architecture/UX/connection guidance is updated. M11 receives the
   unresolved predecessor gates; no unchecked gate may be reported Done.
