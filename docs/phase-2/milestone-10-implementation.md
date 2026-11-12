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
classes and projection choices. The configuration writer/coordinator is not yet
wired to a GUI connection action. Installed-client lock interoperability,
managed/local/project-scope preflight and signed production helper acceptance
remain open; the writer must not be enabled for owner configuration until that
preflight is implemented and verified.

## Implementation review

The [2026-11-13 code review](milestone-10-code-review.md) repairs original-intent
Connect recovery, unambiguous IDs on duplicate-member RPC rejection and bounded
inspection labels. Signed/native/client acceptance remains open.

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

## Work remaining

1. Finish protocol/schema and saturation/cancellation coverage, observation fast
   paths and projection/corpus reconciliation; measure actual client behavior.
2. Finish enrollment crash/race coverage and configuration publication fault
   coverage; verify protected helper fencing across subprocesses and the
   installed client's actual lock protocol before enabling configuration edits.
3. Complete signed-helper cold-start, termination/deadline, hostile-peer and
   credential-rotation client evidence, including the real production identity.
4. Wire token-free configuration/coordinator through verified client/scope
   preflight and typed GUI Connect/Disconnect; expose separate
   saved/verified/client-attached states, repair and relocation paths.
5. Execute the nine actual-client corpus workflows with tool search on/off and
   all-tool recovery parity in the signed isolated harness. Record real visible
   definitions/results, calls/tokens and justified budget exceptions.
6. Complete production-helper/client smoke in a disposable macOS account,
   M9-dependent signing/credential gates and separate-host native attribution
   evidence. Same-Mac synthetic success is insufficient for Local Network consent.
7. Run all required Cargo checks, write the final requirement-by-requirement
   `milestone-10-review.md`, and update current architecture/UX/connection guidance
   and the M11 handoff. No unchecked gate may be reported Done.
