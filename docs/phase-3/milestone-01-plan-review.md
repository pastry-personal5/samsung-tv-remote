# P3-M1 plan review — 2026-11-14

Status: Active — plan corrected; production acquisition and hardware acceptance pending.

The later [implementation record](milestone-01-implementation.md) covers the
semantic foundation and gated diagnostic controller. The findings below
describe the reviewed baseline.

Reviewed the overview, architecture and phase plan against the actual
`TvSessionControl`, Samsung codec/actor, finite `RemoteAction` vocabulary,
production composition and contribution guide. Existing local planning edits
were retained. This review grants no Phase 2 acceptance or production exception.

## Findings and corrections

| Priority | Finding | Correction |
| --- | --- | --- |
| High | The spike assumes a diagnostic path that does not exist. Session control accepts clicks/abort, the actor discards other events, and the domain has no direct-source keys. A Python socket probe would bypass owner/session policy. | Add slice 2a and a concrete private owner-harness contract before hardware work. Keep the live harness explicitly pending; external tooling cannot send TV frames. |
| High | Requiring evidence before any parser can exist makes the first spike circular. | Permit candidate wire shapes only in the gated diagnostic slice; require reviewed captures before production schema freeze. |
| High | “Correlated request” assumes an identifier the TV may not echo. A late response after timeout could satisfy a subsequent experiment. | One outstanding request per uncorrelated family, record the association method, quarantine after timeout until explicit reconnect, fence completions by captured session/target. |
| High | Supported-only availability could block the first catalog request forever. Source keys have no automatic support proof. | Define operation-specific rules: unknown discovery allows explicit guarded acquisition, source configuration uses user intent, text needs a current field/session. |
| High | Launch repetitions could reuse IDs from the previous session or replay an uncertain effect after Wake. | Fresh catalog and A/B aliases per condition; separate explicit reconnect/Wake and per-effect intent. No replay. |
| High | Automating source/launch writes cannot automate proof of the physical panel's state. | Separate machine request/reply observations from human-visible confirmation, and keep absent/silent results inconclusive. The recorder cannot produce a support verdict. |
| Medium | The phase plan says to sanitize IDs, which could redirect a later launch; architecture says to reject them. | Align the phase rule: sanitize display names, reject unsafe IDs without changing identity. |
| Medium | Invalidation can expose a new connection with old current facts if updates are published separately. Historical support and active IME were not explicit storage slots. | Publish invalidation atomically with the transition; retain bounded history separately from live IME; suppress identical-observation revision churn. |
| Medium | The MCP paragraph mentions only the selected record although `inspect_tv` can inspect a saved target. | Project the inspected TV with selection/freshness reasons; never copy the selected TV's evidence. |

## Delivered automation

The [spike workflow](milestone-01-spike-automation.md) specifies ownership,
finite experiments, intent, timeout/correlation handling, initial diagnostic
ceilings, redaction, fixture provenance, human steps and fake-session tests.
The live controller/owner lane is a delivery prerequisite, not an implemented
endpoint or a command that can be run today.

The executable `scripts/p3_spike_record.py` prepares the complete baseline,
reconnect and manual-Wake/reconnect matrix, records one finite observation to a
new file while preserving the prior record, and validates/renders Markdown.
It rejects private/free-form data and never contacts a TV or reads credentials.
Its Python tests run in the Cargo gate through `tests/p3_spike_record.rs`.
No captured fixture or physical result was invented for this review.

## Open acceptance

Phase 2 completion/exception, accepted installed-client MCP budgets, signed
live-harness implementation, actual target-TV observations, reviewed fixtures,
measured parser limits and production M1 implementation remain open. Missing
IME events cannot close M4's fallback decision. Untested source/tuner or
second-app cases need explicit scope treatment rather than assumed success.

## Validation

- `python3 -B tests/test_p3_spike_record.py` — five tests passed.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --all-targets -- -D warnings` — passed.
- `cargo test` — full suite passed with local socket access, including the
  recorder integration and 20 process tests. The first sandboxed attempt failed
  on existing Unix/loopback listener permission restrictions; the rerun passed.
- `git diff --check` and local Markdown file-target/whitespace checks — passed.

These checks validate the offline tooling and repository regressions. No TV,
signed diagnostic controller, protocol capture or installed-client budget
measurement was exercised by this review.
