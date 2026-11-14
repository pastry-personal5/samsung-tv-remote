# Phase 2 unattended validation

The active phase is Phase 2. Its [M11 acceptance matrix](milestone-11-acceptance.md)
separates deterministic integration (D1–D12) from signed/native acceptance
(N1–N11). D1–D12 can run without a physical TV or human interaction, using
temporary profiles, synthetic credentials, fake effects and loopback sockets.
Their existing named coverage is in the
[implementation ledger](milestone-11-implementation.md#requirement-to-test-inventory).

Phase 1's regression coverage remains in the default suite. Phase 3 is planned:
its protocol fixtures depend on future target-TV evidence and its features are
not implemented, so passing Phase 2 tests cannot accept Phase 3.

## Added unattended coverage — 2026-11-14

Base revision: `90b6552e6f1a5ba86b48d663656f46424a40d407`, plus this
working-tree test/documentation diff. No application behavior or storage format
changes. The fixtures do not open production profiles or native Keychain items.

| Row | Added regression | Observable assertion |
| --- | --- | --- |
| D5 | `mixed_client_load_preserves_recovery_events_management_and_cancelled_capacity` in `tests/support/control_integration.rs` | Both principals reach mutation overload with stalled bodies while SSE, MCP POST reads/private waiting and native readiness still progress. Management admission succeeds during overload. Closing bodies restores each role's capacity; a subsequent MCP discovery dispatches exactly once. No remote write or Wake packet occurs. |
| D8 | `blocked_management_worker_retains_ownership_until_durable_disable_finishes` in `src/server/mcp/enrollment.rs` | A channel pauses an admitted worker before persistence. Dropping composition and request-side references cannot release ownership. Only worker completion permits the next owner, which reads the exact durable disabled record. |
| D9 | `crash_after_durable_disable_keeps_leftover_entry_inert_until_explicit_repair` in `tests/server_process.rs` | Kill/reap an isolated foreground owner after durable disable, leaving its managed config entry. The helper emits no credentials or new owner; old Connect retry is stale. A GUI restart preserves disable and changes epoch. Original-key Disconnect retry removes the entry; only explicit fresh Connect creates a new grant. |
| Fixture integrity | `synthetic_tv_fixture_validates_tls_frames_and_redaction_without_hardware` in `tests/tv_fixture.rs` | Runs all eight existing Python fixture checks as part of `cargo test`: real temporary TLS, bounded framing/headers, authorization, reconnect/offline recovery, key validation and diagnostic redaction. Previously this Python suite required a separate invocation. |

The D5 management port is a recorder, so that test proves transport admission,
not actual shutdown settlement. Existing lifecycle/process tests cover Stop.
The D8 schedule is controlled coverage, not a diagnosis of the historical
intermittent failures. D5 does not exhaust every global capacity combination;
D9 does not cover every crash or Connect/helper interleaving. Those broader
review items remain open.

## Commands

Run the canonical [contribution-guide](../contribution-guide.md) checks:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Focused reruns:

```sh
cargo test --test control_http mixed_client_load
cargo test --lib blocked_management_worker
cargo test --test server_process crash_after_durable_disable
cargo test --test tv_fixture
```

Socket/process fixtures require permission to bind temporary loopback/Unix
sockets and launch their own children. The Python TLS fixture also uses
`/usr/bin/openssl` to generate a disposable certificate. None requires a
physical device, local-network discovery, desktop interaction or user consent
dialogs. Execution results are recorded in the
[M11 review](milestone-11-review.md#unattended-validation-follow-up--2026-11-14).

## Remaining acceptance

N1–N4 and N6 have automatable parts but need a final signed installation and
approved native topology; fakes cannot establish prompt, permission, sleep or
upgrade behavior. N8 needs an authenticated isolated installed-client/model
measurement session; a byte proxy cannot prove its token budgets. N9 needs
keyboard/VoiceOver observation, N10 needs a physical TV, and N11 requires the
owner's session-specific consent and hands-on removal/recovery.

N5, N7 and N8's disposable-account subrow remain waived under the recorded
owner decision, with their residual risks. This automated pass neither changes
those waivers nor completes Phase 2.
