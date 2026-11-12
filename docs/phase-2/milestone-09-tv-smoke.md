# P2-M9 optional real-TV smoke runbook

Status: Planned procedure; not run. Execution is **not an M9 exit gate**, per
the owner decision on 2026-11-12. M11 owns the broader hardware matrix. Required
M9 signed identity, Keychain and Local Network checks remain independent.

Run only after the API-only signed bundle passes its signed-platform gates.
The default GUI now uses the API, but native acceptance remains open; source
cutover alone does not establish release readiness. This procedure uses GUI
actions, with no invented future CLI flags.

## Prepare and record

1. Arrange a short interval when nobody else needs the TV. Use a TV you can
   pair, control and wake, with its physical remote available. Record its
   initial power/input/volume locally so those can be restored. Do not run
   Power or Wake unattended or while another client is using it.
2. Record date, source revision and any working-tree fingerprint, macOS
   version, bundle version, signing identity requirement and signature check
   result. Use the documented `make bundle` instructions and launch that
   exact app in Finder. A `cargo run` or development probe result is separate
   evidence. Do not reinstall an older bundle while a newer server is active.
3. Before first new-profile launch, stop the old single-process app and any
   test server using their documented controls. Confirm there is one new
   production owner and that legacy data remains intact. Use the designated
   fresh M9 profile; never erase an existing profile to manufacture first-use
   evidence. If it is already initialized, record first-use as not tested.
4. Record results in a private worksheet: step, expected result, actual result,
   GUI/server epoch transitions, prompt type/count and pass/fail/not-tested.
   Publish only a sanitized copy: no IP/MAC addresses, device names, user
   paths, fingerprints, tokens, Keychain dumps or raw network captures.
   Crop or redact screenshots before adding them to repository evidence.

## Exercise the signed GUI

| Step | Action | Expected observation and limit |
| --- | --- | --- |
| Empty launch | Open the signed GUI with an unused M9 namespace and populated legacy data left in place. | Server attaches automatically; saved list/selection are empty and no TV Connect or legacy import occurs. Record Local Network consent separately from credential and TV approval. |
| Discover/probe | In Settings, discover or enter the TV address locally and probe it. | GUI shows server-provided identity information. Check it against the intended TV; if identity is unexpected, stop rather than accept it. Discovery failure may still allow manual probe, but is recorded as a failure of discovery. |
| First Pairing | Confirm the observed identity explicitly, then approve on the TV. | One Pairing workflow; server reports TV approval and durable save, or precise blocked/partial facts. If consent is denied or times out, inspect the result; do not repeatedly click Pair. Stop for recovery on partial cleanup. |
| Connect | Use explicit Connect if the remote channel is not ready. | Server reports remote readiness. API attachment and TV-channel readiness appear separately. No action is claimed from an admission receipt alone. |
| One action | Use one low-impact visible action, such as a single navigation step in an already open TV menu. | Correlate one intent and its terminal result with the visible effect. `written` means channel write, not verified TV state; record a physical observation separately. Avoid mute, volume or input changes unless their prior state is known. |
| Close/reopen | Close the GUI while the TV channel is ready; reopen the same signed bundle. | Server survives, saved selection restores, GUI receives a fresh atomic attachment and does not reconnect an already ready TV or repeat the prior action. Initial Connect is allowed only for an idle/disconnected saved target. |
| Wake setup | Review the selected device and configure only its correct interface/MAC in Settings. Wait for confirmed save. | Configuration is server-owned. Do not copy addresses into evidence. Missing/invalid configuration blocks Wake instead of sending to a guessed target. |
| Wake | Put the TV into standby with its physical remote; wait until the server observes disconnection. Invoke Wake once after checking the displayed target. | At most one packet per Wake operation. Record packet-send/readiness facts and whether the TV physically wakes separately. The UI cannot prove a packet count alone; cite instrumented evidence only if collected. A packet send without readiness is not full success. |
| Stop/Resume | In Settings choose Stop and confirm shared impact. Wait for settlement, then close/reopen before choosing Resume. | GUI remains usable; sticky Stop survives reopening. Resume explicitly starts/attaches; no old action, Wake or launch Connect is repeated. Stop acknowledgment and completed drain are separate facts. |

If an action becomes uncertain, a stream breaks or the epoch changes, record
the original result/unknown state and inspect before any new explicit intent.
Never use another click as a way to discover whether the first command ran.
Do not cancel a Wake and assume the packet was prevented; record the server's
before/during/after-send cancellation facts if cancellation is tested.

## Finish without destroying evidence

Restore the TV's original state with its physical remote. Choose whether the
server should remain available; explicitly Stop it if this was a temporary
session, since GUI close does not stop it. Keep the new saved profile unless
you intentionally choose Forget and confirm its exact target. A partial Forget
requires the documented recovery path, not manual file or Keychain deletion.

Do not delete legacy data, clear Keychain services or reset privacy settings
as cleanup. Locked/denied Keychain and Local Network revocation/recovery tests
belong in the controlled signed-platform runbook; avoid changing the owner's
whole login Keychain or other apps' permissions for this smoke test.

Record skipped steps and reasons, including unsupported Wake or unavailable
hardware. Link sanitized observations from the eventual M9 completion review
as supplementary evidence. Missing smoke execution does not prevent M9 Done;
it also does not count as passing M11 hardware acceptance.
