# P2-M8 plan review

Date: 2026-11-12. Status: Historical plan review. M8 implementation and
acceptance are recorded in the [completion review](milestone-08-review.md).

Reviewed the M8 overview and architecture against the M7 completion review,
`ControlService`, operation registry, M6 wire draft and M9 cutover plan. This
record reviews a plan; it is not M8 completion evidence or a security audit of
an implemented server. Existing working-tree planning changes were preserved.

## Owner interview

The owner confirmed:

- Keep the early probe in the existing GUI with only enough wiring to prove
  setup, one action and reattach. Complete shipping GUI parity remains M9.
- Show an incompatible-server-build error and require explicit restart.
  Opening the GUI does not automatically replace a running server.
- Automatically connect/reconnect the GUI to the server in the background.
  Use short initial intervals and exponential backoff capped at one hour;
  retain Stop/build/credential gates and never replay mutations. The plan fixes
  the initial delay at one second and resets after 30 healthy seconds.
- Close open event streams on API credential rotation. Reconnect with the new
  credential and a fresh atomic snapshot. Revocation also closes streams.

Earlier decisions remain: same-executable detached `--server`, no login start,
sticky Stop, fresh profile, no legacy import/migration, and isolated fake ports
for M8. Production/signed credential and peer-identity evidence remains M9.

## Findings and plan corrections

| Priority | Gap | Correction in the revised architecture |
| --- | --- | --- |
| P1 | Middleware authentication can race queued owner admission after revocation. | Require a narrow guard or owner-ordered authorization update; test the request paused between verification and admission. Preserve already admitted outcomes. |
| P1 | Stop's admission fence, durability boundary and crash behavior were underspecified. | Fence mutations before the durable marker; success follows durability. A crash after durable Stop preserves it. Marker failure remains visibly blocked. |
| P1 | Lifetime lock ownership can deadlock a launcher trying to Stop or Resume a live owner. | Request live management without holding its lock; wait for settlement/release before Resume or replacement. Use bounded waits and explicit restart. |
| P1 | A challenge from an unknown listener does not authenticate that listener. | Require a reviewed protected-channel transcript before API secrets are sent, including replay and endpoint/profile binding; test the fake trust boundary without bypasses. |
| P1 | A fresh profile alone does not isolate fake credentials or prove restart behavior. | Separate M8 fake and future production namespaces, reject production composition, and prove pair/restart/explicit connect/action in separate server processes. |
| P2 | Full epoch plus counter cannot fit reversibly in one UUID; integer precision was unspecified. | Gate publication on a checked encoding with explicit epoch context and lossless integer fixtures; preserve stored device IDs. |
| P2 | Current receipts/snapshots lack the proposed expiry and recent-operation fields. | Add narrow owner-side projections; capture stable expiry once and keep recent receipts bounded, private and atomic with state. No transport-owned shadow registry. |
| P2 | M6 proposes 64 KiB bodies while M8 proposes 16 KiB. | Reconcile in the first contract slice with maximum-input fixtures before publishing either limit. |
| P2 | Connection/request caps alone cannot guarantee Stop and queries progress. | Reserve finite management/query capacity, keep drain independent and test framing, slow clients, serialization and unauthenticated saturation. |
| P2 | A successful GUI reattach could mask a missing process boundary or persistence path. | Require actual subprocess HTTP evidence and setup through normal authenticated routes; test server restart separately from GUI restart. |

## Original implementation gates

The first implementation slice must freeze wire fixtures, the HTTP dependency
and feature choice, exact limits, ID/integer representation, recovery DTOs,
outcome/status mapping and the minimal service additions. These are technical
implementation decisions requiring recorded rationale and tests, rather than
additional product approval. Current documentation does not claim they are
already settled or implemented. M6 contract/examples and the MCP corpus must
change together when those decisions land.

The access/process slice must settle the authenticated attach transcript,
authorization/admission ordering, capability mapping and executable lifecycle
state machine before a client can submit control operations. Incompatible
management protocols fail visibly; explicit restart does not authorize killing
an unidentified listener or overlapping storage owners.

Each slice produces its own deterministic evidence. M8 completion still
requires the complete API, runnable detached lifecycle and early GUI probe;
none substitutes for the others. Required Cargo checks and a completion review
remain implementation gates. Signed fresh-credential and peer-identity checks
remain M9; full signed session/upgrade integration remains M11.

## Planning validation

For this documentation-only review, validate whitespace, local Markdown links
and consistency with the inspected service types. No application behavior is
changed, and no Cargo or signed-platform result is claimed by this record.
