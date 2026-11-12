# P2-M9 code review

Reviewed on 2026-11-12 against the M8 server baseline `143b072`, through M9
`7dda54d`, with the existing uncommitted planning/runbook changes present.
Review focused on client projection, intent continuations, retained outcomes,
lifecycle management, production composition and native fixture tooling.
This review does not close the milestone's native acceptance gates.

## Fixed findings

| Priority | Finding and consequence | Correction and evidence |
| --- | --- | --- |
| P2 | When selection disappears (for example after Forget or an empty-profile reattachment), a clean Wake editor keeps the previous TV's saved MAC addresses and active interface. The displayed settings no longer match the authenticated selection. | Load the empty Wake configuration when no selected device exists. Preserve unsaved drafts and their conflict state. `cleared_selection_clears_saved_wake_fields_without_discarding_a_dirty_draft` covers both paths and asserts no request is sent. |
| P2 | Typed Wake failures lose their reason in the message feed and are shown as informational. The Power view explicitly claims `SendFailed` means no packet was sent, although the wire projection correctly classifies that result as uncertain. This can encourage another Wake without awareness of the first packet. | Preserve packet-not-sent, packet-possibly-sent and packet-sent facts; include the typed failure reason and report failures as warnings. Power view status and packet step identify uncertain sending. `wake_failure_receipts_preserve_packet_facts_reason_and_warning_in_history` covers send uncertainty, permission denial and failures after sending through local and restored receipts, with zero replay. |
| P2 | Restored history and known terminal outcomes from a previous epoch downgrade all failures and partial cleanup to informational messages. Recovery can hide an actionable failure even though its typed facts are retained. | Preserve warning severity while keeping ordinary history visible as informational. The restored-history regression checks permission and partial cleanup warnings without changing drafts or issuing requests. |

## Validation

Validation passed on the working tree:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test` (325 passed across 22 nonempty suites), including the GUI
  boundary negative corpus and both new fake-client regressions
- `python3 -m unittest discover -s tests -p 'test_m9_tv_fixture.py' -v`
  (8 passed)
- `git diff --check`

The requested test runner did not return results or respond to status
requests; it was stopped and validation ran in the main thread under the
repository's fallback agreement. Initial sandbox runs denied socket binds;
the full Cargo and TLS runs passed with host socket access. Logs are retained
privately at `/private/tmp/m9-review-validation-cargo-test-host.log`,
`/private/tmp/m9-review-validation-clippy.log` and
`/private/tmp/m9-review-validation-fixture-host.log`.
These checks use deterministic fakes and loopback fixtures; they establish no
production permission result. Clippy reports an existing dependency future
compatibility notice for `block v0.1.6`; there are no denied Clippy warnings.

## Remaining acceptance work

The [acceptance audit](milestone-09-acceptance-audit.md) remains authoritative
for the open signed identity, existing API credential access, Keychain failure,
signed GUI workflow/restore and controlled-LAN permission gates. No production
profile, credential, signing identity or permission setting was changed for
this code review. No native screenshot or signed visual acceptance is claimed.
