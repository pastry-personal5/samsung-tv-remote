# P2-M11 native acceptance runbook

Status: Prepared; not execution evidence.

Use the [acceptance matrix](milestone-11-acceptance.md) IDs. The operator records
source revision plus dirty diff identity, bundle version/identifier, executable
SHA-256, Apple signing class/TeamIdentifier, macOS build, Claude version/runtime,
topology, expected/actual result, prompt counts and cleanup per row. Keep
credentials, addresses, fingerprints and device identifiers out of checked-in
records. Record waived rows with their substitute and residual risk.

No automated step in this runbook authorizes resetting credentials, locking
the login keychain, erasing data or claiming TV state from a socket write.
Signed bundles use the production profile. Use an authenticated isolated
`CLAUDE_CONFIG_DIR` for the corpus; do not copy owner authentication into it.
Follow [M10 topology](milestone-10-architecture.md#acceptance-topology).

## Candidate, lifecycle and upgrade — N1–N6

Setup: finish pending TV actions, retain the currently working signed bundle
and record its path and provenance. Build a preserved candidate using
`make bundle-candidate`; inspect its Apple signature and artifact hash before
installation. A build or signature inspection is not native acceptance.
Record baseline Stop/enrollment and saved-TV facts without exporting secrets.

1. N1: cold-start GUI and configured helper together. Observe one authenticated
   epoch/owner, matching endpoint and no routine Keychain prompt. Test hostile
   peers/listeners only in the isolated fixture topology; record fail-closed
   behavior separately from actual production identity. Cleanup: settle fixture
   owners and remove only their temporary directories.
2. N2: keep an MCP read active, close/reopen GUI, then explicitly Stop/Resume.
   Observe MCP surviving GUI exit, both clients respecting Stop and a new
   authenticated snapshot after Resume. Repeat incompatible restart and crash
   only in a review session with interrupted effects recorded unknown; never
   resubmit uncertain actions automatically. Cleanup: restore intended running
   state and explicitly verify both clients.
3. N3: capture the operation key and guarded target for one safe intent, sleep
   and wake with a broken event stream. Observe bounded resnapshot, no retarget,
   spontaneous Resume or second write. Repeat with an intentionally stopped
   profile. Cleanup: explicitly Resume only when the operator wants it running.
4. N4: use the installer for active, detached and intentionally stopped owner
   variants. Exercise compatible and incompatible old builds. Observe settlement
   before replacement, retained bundle, data/trust/API/TV credential continuity
   without routine prompts and honest interrupted outcomes. Roll back by
   installing the retained bundle through the same installer, then restore the
   candidate through that installer. Record any launch blocked by publication
   and verify explicit retry works after installation. Cleanup: keep the newest
   recovery bundle and manually remove older retained bundles only after review.
5. N5: waived. Attach deterministic cross-UID rejection and runtime-directory
   results. Residual risk: real fast-user-switching is unobserved.
6. N6: use approved isolated credential fixtures for denied, locked and
   interaction-required cases. Observe visible blocked state and no unattended
   prompt loop. Count routine prompts separately from Gatekeeper, Local Network,
   legacy ACL/unlock, TV Pairing and optional Claude consent. Cleanup: restore
   only fixture permissions/items. Do not lock or reset the owner's keychain.

## Client and model measurements — N7–N8

N7 is waived: attach same-Mac protocol evidence and, when available, N10
observed traffic. Residual risk: separate-host native attribution and
deny/revoke/restore remain unobserved.

N8 setup: final signed isolated synthetic-effect harness, fresh explicit Claude
configuration directory, approved authentication and supported client version.
Record model, runtime, cache and tool-search setting. Do not write dynamic
Authorization output to a transcript, fixture file or diagnostic log.

Run all nine M6 corpus workflows with search enabled and disabled, including
all-tools-upfront fallback, all 15 tools, slow private lookup, changed guards,
confirmation, unknown/crash-lost outcomes, header refresh and revoked access.
Capture the definitions actually provided to the model, both result channels,
duplicated content, calls and model turns using the client's available measured
usage. If the client cannot expose that evidence, mark the gate blocked.
`measure_mcp_corpus.py` is only a byte/4 proxy and cannot fill token fields.

Pass conditions: catalog/instruction ≤1,900 tokens; ordinary receipt ≤180;
read summary ≤300; exceptional fingerprint/cleanup ≤450; workflow ≤1,700 with
search or ≤2,800 upfront, subject to the M6 detailed workflow rules. An overrun
stops at the owner's trim/re-baseline decision. Do not silently change budgets
or remove safety guards. Cleanup: revoke the isolated enrollment, Stop and
settle its owner, remove only its temporary client entry/profile. N8's
disposable-account production smoke is waived; this does not waive model data.

## Accessibility and physical TV — N9–N10

N9 setup: final artifact, keyboard-only input; schedule VoiceOver separately.
Traverse Power, Remote and Settings, setup/discovery/manual probe, selection,
Pair/re-pair, Wake/cancel, Forget, server controls and MCP Check/Connect/Verify/
Disconnect/repair. Exercise busy, empty, blocked/error and recovery states.
Observe reachable controls, visible focus, Tab/Shift-Tab order, activation,
Escape/confirmation cancellation and focus restoration. In the VoiceOver
session record names, roles, states and status/outcome announcements. Pass
only observed flows. VoiceOver remains blocked until that session occurs.
Cleanup: dismiss transient dialogs and restore the operator's selection.

N10 setup: owner-present physical target TV, safe representative actions and
recorded desired final power/selection state. Exercise discovery/manual probe,
full fingerprint confirmation, Pair/re-pair, selection, reconnect, representative
keys, network-loss recovery and Wake/cancel. Record one packet/write per intent
separately from observed panel/audio/player/power response. Forget/re-pair only
with a working recovery path and owner participation. Cleanup: restore network,
pairing, selection and the agreed final power state. Unobserved behavior stays
blocked; readiness alone is not physical response evidence.

## Hands-on removal and recovery — N11

This row requires explicit consent at the session because it changes the
owner's signed installation. Present the following sequence for approval before
performing it. Retain the exact working signed bundle for reinstall first.

1. Record saved-TV count/selection, running/stopped state and separate MCP
   configuration/enrollment/verification facts. Finish pending work.
2. In Settings → Servers choose **Disconnect Claude Code** and confirm.
   Observe acknowledged disabled enrollment and exact-entry removal. For a
   controlled changed-entry variant, use an isolated client configuration;
   observe conflict with enrollment disabled and unrelated bytes retained.
3. Choose **Stop** and confirm the all-client effect. Require settled owner
   evidence. A marker observed after a lost reply alone does not prove settlement.
   Retry/recheck before proceeding if outcome is unknown.
4. Move only the installed application bundle to Trash. Keep profiles and
   Keychain items. Observe no detached owner/helper resurrection. Repeat with
   an already stopped owner, where Disconnect must use offline management
   without clearing Stop.
5. Reinstall the retained signed bundle with the installer. Before any explicit
   Resume/Connect, verify Stop persists and helper access remains denied even
   with a leftover entry. Reinstall alone must restore neither service nor grant.
6. Recovery: explicitly **Resume**, **Connect Claude Code**, then **Verify**.
   Restore the original selection and confirm retained TV records and usable
   approved credentials. A changed entry needs explicit inspection/repair, never
   overwriting unrelated settings. Verify is endpoint evidence; separately
   observe actual Claude attachment. Record cleanup completion or actionable
   partial recovery, and leave the app in the owner's requested final state.

If a step fails, retain both bundles and data, report the acknowledged progress
and stop destructive steps. Do not empty Trash or erase profiles/credentials.
