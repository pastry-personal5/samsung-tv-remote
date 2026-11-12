# P2-M6 isolated macOS test runbook

Status: human execution pending. Clean-restart instructions refreshed 2026-11-12.
This is the Phase 2 M6 Keychain/platform experiment; P1-M6 is archived.

The [acceptance record](milestone-06-acceptance.md) defines the boundary:

- The focused old-GUI Keychain experiment is required for M6.
- The LaunchAgent comparison is optional research on a rejected alternative.
- Production server, privacy, and Claude workflows belong to M8–M11.
- One explicit migration approval is allowed; routine access must have zero
  prompts. The prepared probes are noninteractive and cannot request that
  one-time approval or demonstrate a complete migration/rollback flow.

Preparing a handoff verifies and copies bundles. It does not run a probe,
register an agent, read a Keychain item, or complete M6.

For a restart, follow section 1 even if test apps are already installed. It
removes the previous M6 installation and disposable pairing state in
`m6probe`, then installs the staged bundles again. Complete preparation in
the main account before removing anything in `m6probe`.

## 0. Prepare the handoff in the usual desktop account

In the main desktop account, open Terminal and check that the dedicated
account still exists and is absent from the `admin` group's membership:

```sh
cd /Volumes/Work_Volume/samsung-tv-remote &&
id m6probe
```

Stop if the account is missing or is an administrator. Account setup is a
separate manual prerequisite. Then initialize the saved signing identity,
build a fresh Keychain probe, and stage the required bundles. Run the whole
block; a failure stops the following commands:

```sh
zsh scripts/create_local_signing_identity.sh &&
zsh scripts/build_m6_signed_keychain_probe.sh &&
zsh scripts/stage_m6_human_handoff.sh --keychain-only
```

The identity script reuses `Samsung TV Remote Local Test` in this account's
login Keychain, or creates it if absent, and writes
`target/local-signing-identity.txt`. An environment variable alone does not
configure the probe builder. Do this in the main account, not `m6probe`.

`--keychain-only` uses the newest probe bundle
under `target/m6-keychain.*`, plus `target/bundle/Samsung TV Remote.app`.
The optional LaunchAgent is not needed for this run. The script checks each
bundle's identity and deep signature before and after copying. It prints the
actual `stage='…'` command and creates a copy of this runbook with that path
already filled in.
The handoff is readable by the test account and writable only by its preparer.

Use that printed path; an earlier path such as
`/private/tmp/m6probe-bundles.66DVb8` may have disappeared after a reboot or
cleanup. Open `HANDOFF.txt` and `milestone-06-human-runbook.md` in the new
directory before switching users. Neither account needs access to the
repository to follow the staged instructions. The three directly linked
context records are also included; their further links refer to the repository.

If the legacy GUI bundle is missing, build it in the main account only if
this checkout still builds the legacy GUI with direct Keychain pairing:

```sh
make bundle &&
zsh scripts/stage_m6_human_handoff.sh --keychain-only
```

To include the optional LaunchAgent comparison, build that probe and stage
without `--keychain-only`:

```sh
zsh scripts/build_m6_signed_launch_agent_probe.sh &&
zsh scripts/stage_m6_human_handoff.sh
```

The probe builders now publish their discoverable directories only after
signature verification succeeds, and remove their own unfinished builds on
failure. Existing incomplete directories are not repaired or deleted by
staging. A `code has no resources but signature indicates they must be
present` error can mean a resource seal is missing from an unfinished build;
rebuild the named probe. If rebuilding fails, stop at that error; do not
re-sign the bundle ad hoc or bypass verification for the actual Keychain
experiment.

The probe builders use `target/local-signing-identity.txt`; `make bundle` also
supports `SAMSUNG_TV_CODESIGN_IDENTITY`. A signature failure is a stop point:
resolve signing in the preparing account, then stage again. Do not bypass
verification or substitute an unsigned app. Preserve the old signed GUI for
this experiment; a future server-client GUI does not create the legacy item.
Its bundle ID and valid signature alone do not prove that it still owns
legacy pairing. If that GUI is no longer available, report a blocked
prerequisite rather than substituting a newer client.

To choose specific existing builds, pass Keychain and GUI paths after
`--keychain-only`, or all three paths in LaunchAgent, Keychain, GUI order
without the flag; see the staging script's `--help` output.

## 1. Uninstall the previous M6 run and reinstall in m6probe

Log in through the macOS login screen as **`m6probe`**, then open Terminal.
Do not substitute `sudo -u m6probe` from another user's session: the desktop
session and login Keychain are part of the test. Do not sign this account in
to an Apple Account or pair a personal/in-use TV. Use a disposable spare TV
or a fake TV that can complete the old GUI's pairing flow. There is no
pairing-capable fake bundled in this handoff; if neither target is available,
report the Keychain test as blocked.

This reset removes the three test app copies under `~/Applications`, the
saved devices/trust/recovery files under
`~/Library/Application Support/Samsung TV Remote`, the optional agent's
log directory, and disposable legacy pairing items from this account's
Keychain. Save any sanitized results you need before starting. The macOS
account, login Keychain itself, signing certificates and OS permission
decisions remain in place; this is not a fresh-account permission test.

### 1a. Check the account and replacement bundles

Use the `stage` assignment printed during the latest preparation. In the
new staged copy of this runbook, it is already filled in below. Run this
block before uninstalling anything:

```sh
stage='/private/tmp/m6probe-bundles.REPLACE_ME'
(
set -eu
test "$(id -un)" = 'm6probe' || { echo 'Stop: use the m6probe desktop account.' >&2; exit 1; }
test -d "$stage"
for name in 'M6 Keychain Probe.app' 'Samsung TV Remote.app'; do
    codesign --verify --deep --strict "$stage/$name"
done
)
```

Stop on any error. If the handoff has disappeared or its signature is
invalid, prepare a new one in the main account first. Do not substitute an
unsigned bundle. The `stage` assignment stays in this Terminal; set it again
from `HANDOFF.txt` after opening a new Terminal or logging in again.

Quit Samsung TV Remote and any running Keychain probe. Use Activity Monitor
in `m6probe` if a probe is still running. Leave the GUI closed during reset
so it cannot reconnect or rewrite the state being removed.

### 1b. Unregister an earlier optional LaunchAgent

Skip this subsection if the optional LaunchAgent app was never installed.
If it is installed, inspect it before deleting its bundle:

```sh
(
set -eu
test "$(id -un)" = 'm6probe' || { echo 'Stop: use the m6probe desktop account.' >&2; exit 1; }
agent="$HOME/Applications/M6 LaunchAgent Probe.app/Contents/MacOS/m6-agent-controller"
codesign --verify --deep --strict "$HOME/Applications/M6 LaunchAgent Probe.app"
"$agent" status
)
```

For `notRegistered`, continue to section 1c. For `enabled` or
`requiresApproval`, unregister it, then check the status:

```sh
(
set -eu
test "$(id -un)" = 'm6probe' || { echo 'Stop: use the m6probe desktop account.' >&2; exit 1; }
agent="$HOME/Applications/M6 LaunchAgent Probe.app/Contents/MacOS/m6-agent-controller"
"$agent" unregister
"$agent" status
)
```

Continue only when it reports `notRegistered`. If `notFound` appears and
this app was never registered, continue; otherwise stop and resolve the
earlier registration before deleting anything. A missing controller or
failed unregister is also a stop point if an agent was previously
registered. Never delete a registered agent's bundle or its log first.

### 1c. Remove the previous disposable Keychain items

In **Keychain Access inside `m6probe`**, unlock that account's login Keychain
using the native UI, then search for `dev.samsungtvremote.local.pairing`.
Delete every disposable M6 generic-password item with that exact service,
including leftovers from attempts whose saved device record is already gone.
Check the item's attributes to confirm the service and its `dev_<decimal>`
account; do not show or copy the password. Search again and confirm no M6
item for that service remains. If there are no matching items, continue.

All items being removed must belong to disposable tests in this dedicated
account. Stop if an item belongs to an in-use TV or cannot be deleted. Do
not delete the login Keychain itself, certificates or other services. Record
cleanup prompts separately from the new probe's read/mutation prompt counts.
Removing app files alone does not remove these Keychain items.

### 1d. Remove the installed test apps and their files

After section 1b is complete, Keychain cleanup is complete, and all test
processes are quit, run the whole block. It is safe to repeat when these
paths are already absent. The handoff directory is kept for reinstallation.

```sh
(
set -eu
test "$(id -un)" = 'm6probe' || { echo 'Stop: use the m6probe desktop account.' >&2; exit 1; }
test -d "$stage"
for name in 'M6 Keychain Probe.app' 'Samsung TV Remote.app'; do
    codesign --verify --deep --strict "$stage/$name"
done
# Refuse redirected parent directories before removing any test files.
for directory in "$HOME/Applications" "$HOME/Library" "$HOME/Library/Application Support"; do
    test ! -L "$directory" || { echo 'Stop: a test directory parent is a symlink.' >&2; exit 1; }
done
rm -rf -- "$HOME/Applications/M6 Keychain Probe.app" \
    "$HOME/Applications/M6 LaunchAgent Probe.app" \
    "$HOME/Applications/Samsung TV Remote.app" \
    "$HOME/Library/Application Support/Samsung TV Remote" \
    "$HOME/Library/Application Support/M6LaunchAgentProbe"
echo 'Previous M6 test apps and application data removed.'
)
```

Stop on any error. After success, clear the old test variables in this
Terminal so the next run cannot reuse a deleted item's ID:

```sh
unset M6_ISOLATED_KEYCHAIN_ACCOUNT M6_SYNTHETIC_DEVICE_ID M6_SYNTHETIC_DEVICE_UUID
unset probe helper agent log
```

### 1e. Install the staged apps again

Run the whole block in the same `m6probe` Terminal:

```sh
(
set -eu
test "$(id -un)" = 'm6probe' || { echo 'Stop: use the m6probe desktop account.' >&2; exit 1; }
test -d "$stage"
mkdir -p "$HOME/Applications"
for name in 'M6 Keychain Probe.app' 'Samsung TV Remote.app'; do
    test ! -e "$HOME/Applications/$name" || { echo 'Stop: repeat section 1 reset before installing.' >&2; exit 1; }
    codesign --verify --deep --strict "$stage/$name"
done
for name in 'M6 Keychain Probe.app' 'Samsung TV Remote.app'; do
    ditto --noacl "$stage/$name" "$HOME/Applications/$name"
    codesign --verify --deep --strict "$HOME/Applications/$name"
done
)
```

Stop on any error. A partial install can be reset by repeating section 1.
The optional LaunchAgent is installed only in section 3.

After a successful copy, record the OS and code identities:

```sh
sw_vers -productVersion
sw_vers -buildVersion
for name in 'M6 Keychain Probe.app' 'Samsung TV Remote.app'; do
    codesign -d --verbose=2 "$HOME/Applications/$name" 2>&1
done
codesign -d --verbose=2 "$HOME/Applications/M6 Keychain Probe.app/Contents/Helpers/m6-keychain-probe-helper" 2>&1
```

Keep the code identifier, signing authority and CDHash for the old GUI,
the main probe and its helper separately. `HANDOFF.txt` identifies the
repository HEAD at staging; existing binaries may predate it, so record a
binary build revision as unknown unless independently established. Redact
personal account names, email addresses and paths from any shared output.
Never include passwords, pairing tokens, TV addresses, or raw `devices.json`.

## 2. Check the old GUI's Keychain item (required)

Open the freshly installed old GUI. It must have no saved TVs after reset;
if old entries remain, quit and repeat section 1. Complete a new pairing
with the disposable target, then quit the GUI. Record any TV approval and
macOS prompts separately:

```sh
open -n "$HOME/Applications/Samsung TV Remote.app"
```

Print only the saved device IDs from the test account's preferences:

```sh
python3 - <<'PY'
import json
from pathlib import Path
path = Path.home() / 'Library/Application Support/Samsung TV Remote/devices.json'
for device in json.loads(path.read_text())['devices']:
    print(device['id'])
PY
```

If the file is absent, pairing did not save an item; stop and report that
prerequisite. A clean run with one new disposable pairing should have one
ID. If several IDs are present, stop and reconcile the setup before
continuing. Do not reuse an ID from the run that was reset.

Replace `DEVICE_ID_FROM_ABOVE` with that disposable device's exact
`dev_<decimal>` ID. Each probe requires the logged-in test account and the
exact synthetic device ID:

```sh
export M6_ISOLATED_KEYCHAIN_ACCOUNT='m6probe'
export M6_SYNTHETIC_DEVICE_ID='DEVICE_ID_FROM_ABOVE'
probe="$HOME/Applications/M6 Keychain Probe.app/Contents/MacOS/m6-keychain-probe"
helper="$HOME/Applications/M6 Keychain Probe.app/Contents/Helpers/m6-keychain-probe-helper"
"$probe" validate-input "$M6_SYNTHETIC_DEVICE_ID" &&
    "$probe" read "$M6_SYNTHETIC_DEVICE_ID"
echo "exit=$?"
```

`validate-input` checks the arguments and isolation variables without accessing
Keychain. Stop if it fails. Do not convert the saved `dev_…` ID into a UUID:
the exact saved string is the Keychain account. Older probes that print
`<device-uuid>` in their usage must be rebuilt and staged again in the main
account. Then repeat section 1 for a clean restart and pair again before
using a new saved ID. Do not copy a new probe over the installed apps midway
through a case.

Record the numeric status, byte count (if present), exit code, and prompt
count. A success is `read: status=0 bytes=…` with exit code zero. Run the helper
as a **separate case**, then repeat successful reads once to check routine
access:

```sh
"$helper" read "$M6_SYNTHETIC_DEVICE_ID"
echo "exit=$?"
```

Nonzero results, including denied, locked or interaction-required access,
end that case. Record them without retry loops. The other identity can still
be tested separately. If either noninteractive read unexpectedly prompts,
cancel it and record the prompt; do not grant blanket access just to pass.
A successful probe read proves access by that candidate identity only; it
does not prove access by the future signed server.

### Locked/denied cases and one-time approval

Before destructive tests, use Keychain Access **inside `m6probe`** to lock only
that account's login Keychain. Run each `read` once and record its result and
prompts, then unlock using the native UI. Never put a Keychain password in a
command, environment variable, report, or shell history. Repeat a successful
unlocked read once to record recovery. If this macOS setup cannot lock the
test Keychain, record the case as deferred with the reason.

The probes disable interaction. An access denial or interaction-required
status is useful evidence, not a reason to enable prompts or edit the item's
access rules. If there is no denial case, record it as untested; do not claim
that a locked-Keychain result proves user-denial behavior. A one-time
user-assisted migration approval needs a separately implemented path. Record
that limitation and the exact M9 identity/cutover blocker; this runbook
cannot demonstrate that path or an upgrade/rollback.

### Replace and delete the disposable item

Only for an identity whose unlocked read succeeded, and only after collecting
the read/locked results, run its destructive case:

```sh
"$probe" replace-and-delete "$M6_SYNTHETIC_DEVICE_ID"
echo "exit=$?"
```

Expected output: initial read succeeds, `replace: status=0`,
`verify: status=0 matches-synthetic=true`, `delete: status=0`, then
`read: status=-25300` (item absent). The command exits zero for that expected
final absence. Stop if any other result occurs. Once replacement succeeds,
a later failure can leave the fixed test value in Keychain; the disposable
pairing is then unusable until deliberately recreated. Do not treat it as
an unchanged old-GUI item or continue another case with it.

This replaces the token with a fixed test value and deletes it. To test the
helper's mutation, first use the old GUI's Forget flow for the disposable
saved TV, then pair the disposable target again through that GUI,
quit it, print the current device ID again and update
`M6_SYNTHETIC_DEVICE_ID`. Confirm the helper can read this recreated item,
then run:

```sh
"$helper" replace-and-delete "$M6_SYNTHETIC_DEVICE_ID"
echo "exit=$?"
```

Never run these commands on an in-use TV item. Do not launch the GUI again
after deleting the item unless deliberately setting up another disposable
case. Record any unrun mutation as blocked/deferred rather than passed.

## 3. Compare the signed LaunchAgent (optional)

Skip this section if only collecting required M6 Keychain evidence. It needs
a full handoff prepared without `--keychain-only`. In the `m6probe` desktop,
copy the optional bundle before inspecting status (set `stage` from
`HANDOFF.txt` again after a new login):

```sh
(
set -eu
test "$(id -un)" = 'm6probe'
name='M6 LaunchAgent Probe.app'
test ! -e "$HOME/Applications/$name"
codesign --verify --deep --strict "$stage/$name"
ditto --noacl "$stage/$name" "$HOME/Applications/$name"
codesign --verify --deep --strict "$HOME/Applications/$name"
codesign -d --verbose=2 "$HOME/Applications/$name" 2>&1
)
```

Stop on any error; clean up an earlier registered agent before replacing its
bundle. Now inspect status before registration:

```sh
agent="$HOME/Applications/M6 LaunchAgent Probe.app/Contents/MacOS/m6-agent-controller"
log="$HOME/Library/Application Support/M6LaunchAgentProbe/starts.log"
"$agent" status
if test -f "$log"; then wc -l "$log"; else echo '0 starts'; fi
```

Record all three status lines and the baseline count. If `notFound`, stop
**this optional section** and do not register; workspace and temporary-path
tests already saw that result. Only start a fresh registration from
`notRegistered`. An already enabled/requiresApproval service needs cleanup
from its earlier experiment first.

```sh
"$agent" register
"$agent" status
if test -f "$log"; then wc -l "$log"; else echo '0 starts'; fi
```

If `requiresApproval`, approve only this test agent in System Settings, then
repeat status and line count. Record that OS approval separately from any
Keychain prompts. Wait for the one-shot helper's log entry before recording
the count. Log out, log back in, open a **new Terminal**, and reassign the
variables:

```sh
agent="$HOME/Applications/M6 LaunchAgent Probe.app/Contents/MacOS/m6-agent-controller"
log="$HOME/Library/Application Support/M6LaunchAgentProbe/starts.log"
"$agent" status
if test -f "$log"; then wc -l "$log"; else echo '0 starts'; fi
```

A new line means the registered agent ran at login. Now unregister:

```sh
"$agent" unregister
"$agent" status
if test -f "$log"; then wc -l "$log"; else echo '0 starts'; fi
```

Log out and back in once more, reassign `agent` and `log` in the new Terminal,
and repeat status/count. The count must not increase after unregistering.
If a later step fails after registration, unregister before leaving the test
account. Do not delete the installed LaunchAgent bundle before unregistering.
The helper log contains only timestamps and process IDs.

## What to report and clean up

Use one row per invocation, with no secrets:

| Case / exact command | Expected | Actual status / exit code / byte count | macOS prompts | Pass / fail / blocked / deferred and reason |
| --- | --- | --- | --- | --- |
| Main read; helper read; repeated reads | Unlocked reads succeed without prompts | | | |
| Main/helper locked read and unlocked recovery | Locked access fails closed; recovery recorded | | | |
| Denied / interaction-required access | Fails closed without a retry loop | | | |
| Main/helper replace-and-delete, separate items | Verified replacement, deletion, final item absent | | | |
| Optional agent status/login/unregister counts | Login starts only while registered | | | |

Also record date, handoff revision from `HANDOFF.txt`, OS version/build,
bundle identifiers/signing identity/CDHashes, isolated account (use the test
label only), target type (disposable spare or pairing-capable fake), and TV
approval count. No-prompt staging is not a Keychain prompt measurement.

Return sanitized results before marking any gate complete. After collecting
evidence and confirming no further case needs the disposable item, quit the
GUI and follow sections 1b through 1d to unregister the optional agent,
remove leftover disposable Keychain items and uninstall the test apps/data.
Leave the test Keychain unlocked. For another run, continue with section 1e,
pair again and use the new saved ID; if the handoff is gone, prepare a fresh
one in the main account first. Record any earlier OS permission grants as
existing state, not a new consent measurement.

The focused Keychain evidence and process-choice acceptance remain open
until reviewed. Final server identity, upgrade/migration and rollback remain
M9 gates. See [platform evidence](milestone-06-platform-evidence.md) and the
[requirement review](milestone-06-review.md).
