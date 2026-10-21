# P2-M6 isolated macOS test runbook

Status: Ready for a human to run in the dedicated `m6probe` desktop account.
The probes are signed, but the LaunchAgent has not been registered and the
Keychain probe has not accessed an item. These experiments provide platform
evidence; they do not implement or validate the future control server.

## What is ready and what remains

| Work | Human action or dependency |
| --- | --- |
| Signed LaunchAgent comparison | Log into the `m6probe` desktop, stage the signed bundle, check `status`, then test registration and later login only if the service is recognized. Workspace and `/private/tmp` runs returned `notFound`. |
| Old-GUI Keychain access | Use a disposable Samsung TV or a pairing-capable fake TV to create a synthetic item through the old signed GUI in `m6probe`. Read it with both signed probe executables. Replace/delete only the synthetic item, recreating it between destructive runs. |
| Locked, denied, upgrade and prompt behavior | Run after basic old-GUI item access works. Record status codes and prompt counts; never copy a token to a log or issue. The probe's new bundle identity does not prove future same-executable server access. |
| Local Network attribution and fast user switching | Require signed GUI and later signed server runs in actual desktop sessions; record OS prompts and behavior for two macOS users. |
| Real HTTP/MCP and server security | A production server is not implemented in M6. Signed `/mcp`, authenticated endpoint, Claude Code workflows, rotation, operation drain, crash recovery and rollback need implementation and later runtime tests. The design checklist remains open. |

The current `m6probe` account exists and `id m6probe` did not list the `admin`
group. The temporary handoff directory below contains three signed bundles;
it may disappear after a reboot or cleanup. If it is missing, rebuild with
`zsh scripts/build_m6_signed_launch_agent_probe.sh` and
`zsh scripts/build_m6_signed_keychain_probe.sh` from the repository, then copy
the resulting bundles and `target/bundle/Samsung TV Remote.app` into a new
owner-created, world-readable temporary handoff directory. Verify each copy
with `codesign --verify --deep --strict` before using it.

## 1. Stage bundles in the test desktop account

Log into the **macOS desktop** as `m6probe` and open Terminal there. Do not
substitute `sudo -u m6probe` from another user's session: the login session
and Keychain context are part of the experiment. Do not sign this account into
an Apple Account or use it for a real TV pairing.

```sh
id -un
stage='/private/tmp/m6probe-bundles.66DVb8'
test -d "$stage"
mkdir -p "$HOME/Applications"
ditto "$stage/M6 LaunchAgent Probe.app" "$HOME/Applications/M6 LaunchAgent Probe.app"
ditto "$stage/M6 Keychain Probe.app" "$HOME/Applications/M6 Keychain Probe.app"
ditto "$stage/Samsung TV Remote.app" "$HOME/Applications/Samsung TV Remote.app"
codesign --verify --deep --strict "$HOME/Applications/M6 LaunchAgent Probe.app"
codesign --verify --deep --strict "$HOME/Applications/M6 Keychain Probe.app"
codesign --verify --deep --strict "$HOME/Applications/Samsung TV Remote.app"
```

The first command must print `m6probe`. Stop if the directory check or any
signature check fails. Do not send passwords, pairing tokens or TV addresses
when reporting results.

## 2. Compare the signed LaunchAgent

```sh
agent="$HOME/Applications/M6 LaunchAgent Probe.app/Contents/MacOS/m6-agent-controller"
"$agent" status
```

Report all three lines: bundle ID, plist presence and service status. If the
status is `notFound`, **stop here and do not register**; this is the result
already observed from the workspace and `/private/tmp`. If it is
`notRegistered`, continue:

```sh
"$agent" register
"$agent" status
log="$HOME/Library/Application Support/M6LaunchAgentProbe/starts.log"
if test -f "$log"; then wc -l "$log"; else echo '0 starts'; fi
```

If status is `requiresApproval`, approve this test agent in macOS System
Settings, then repeat `"$agent" status` and the line count. Record any
approval prompt. Log out of `m6probe`, log back in, and run the following in
a **new Terminal**; shell variables from before logout no longer exist:

```sh
agent="$HOME/Applications/M6 LaunchAgent Probe.app/Contents/MacOS/m6-agent-controller"
log="$HOME/Library/Application Support/M6LaunchAgentProbe/starts.log"
"$agent" status
if test -f "$log"; then wc -l "$log"; else echo '0 starts'; fi
```

A new line would show that the registered agent ran at login. Then
unregister and inspect status:

```sh
"$agent" unregister
"$agent" status
if test -f "$log"; then wc -l "$log"; else echo '0 starts'; fi
```

Log out and back in once more. Reassign `log` in the new Terminal, then
confirm the line count did not increase.
If registration succeeded but a later step fails, run `"$agent" unregister`
before leaving the test account and report the failure. The log contains only
timestamps and PIDs.

## 3. Check the old GUI's Keychain item

Do this only after confirming a **disposable pairing target**. Open the copied
old signed GUI, complete Pairing, and quit it. The target must be a spare TV
or a fake TV that actually completes the old GUI's pairing flow.

```sh
open -n "$HOME/Applications/Samsung TV Remote.app"
```

In Terminal, print only saved device IDs from the test account's preferences:

```sh
python3 - <<'PY'
import json
from pathlib import Path
path = Path.home() / 'Library/Application Support/Samsung TV Remote/devices.json'
for device in json.loads(path.read_text())['devices']:
    print(device['id'])
PY
```

Set `UUID_FROM_ABOVE` to the synthetic device's ID, then read the exact item
with both signed identities. The probe requires the current login name and
UUID in environment variables before it accesses Keychain.

```sh
export M6_ISOLATED_KEYCHAIN_ACCOUNT="$(id -un)"
export M6_SYNTHETIC_DEVICE_UUID='UUID_FROM_ABOVE'
probe="$HOME/Applications/M6 Keychain Probe.app/Contents/MacOS/m6-keychain-probe"
helper="$HOME/Applications/M6 Keychain Probe.app/Contents/Helpers/m6-keychain-probe-helper"
"$probe" read "$M6_SYNTHETIC_DEVICE_UUID"
"$helper" read "$M6_SYNTHETIC_DEVICE_UUID"
```

Record status codes, byte counts and every OS prompt. A successful read
means only that this probe identity could access this synthetic item; it does
not prove silent access from the future signed server.

Only after both reads, and only for the disposable item, run one destructive
case:

```sh
"$probe" replace-and-delete "$M6_SYNTHETIC_DEVICE_UUID"
```

This replaces the token with a fixed synthetic string, verifies it, and
deletes the item. Recreate a disposable old-GUI pairing before running the
helper's `replace-and-delete`; obtain the current UUID again and update
`M6_SYNTHETIC_DEVICE_UUID`. Never run either destructive command for a real TV
item. If a read reports denied, locked, interaction-required or another
nonzero status, record it and stop that case rather than retrying in a loop.

## Report back

Send the LaunchAgent `status` lines, start-log line counts before and after
login/unregister, Keychain probe status lines and prompt counts, and whether
the test target was a spare TV or a pairing-capable fake. Redact host names,
addresses and all credentials. The remaining runtime gates are listed in the
[platform evidence](milestone-06-platform-evidence.md) and
[requirement review](milestone-06-review.md).
