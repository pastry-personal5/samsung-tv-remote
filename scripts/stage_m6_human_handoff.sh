#!/bin/zsh
# Copies verified, already-built bundles for a separate desktop account.
set -euo pipefail

repo_root="${0:A:h:h}"
usage='usage: zsh scripts/stage_m6_human_handoff.sh [--keychain-only] [<launch-agent.app> <keychain.app> <gui.app>]
       With --keychain-only, pass only <keychain.app> <gui.app>, or no paths.'
if [[ "${1:-}" == '--help' && $# == 1 ]]; then
    print -r -- "$usage"
    exit 0
fi
keychain_only=0
if [[ "${1:-}" == '--keychain-only' ]]; then
    keychain_only=1
    shift
fi
expected_paths=$(( 3 - keychain_only ))
if (( $# != 0 && $# != expected_paths )); then
    print -ru2 -- "$usage"
    exit 2
fi

if (( $# != 0 )); then
    bundles=("$@")
else
    # Prefer the newest bundle from each builder; never fall back silently if
    # that build is incomplete or fails verification.
    keychain_bundles=("$repo_root"/target/m6-keychain.*/'M6 Keychain Probe.app'(N/om))
    if (( ${#keychain_bundles} == 0 )); then
        print -ru2 -- 'Missing Keychain probe. Run zsh scripts/build_m6_signed_keychain_probe.sh first.'
        exit 2
    fi
    bundles=("${keychain_bundles[1]}" "$repo_root/target/bundle/Samsung TV Remote.app")
    if (( ! keychain_only )); then
        launch_bundles=("$repo_root"/target/m6-launch-agent.*/'M6 LaunchAgent Probe.app'(N/om))
        if (( ${#launch_bundles} == 0 )); then
            print -ru2 -- 'Missing LaunchAgent probe. Build it or use --keychain-only for the required M6 experiment.'
            exit 2
        fi
        bundles=("${launch_bundles[1]}" "${bundles[@]}")
    fi
fi
names=('M6 LaunchAgent Probe.app' 'M6 Keychain Probe.app' 'Samsung TV Remote.app')
rebuild_commands=('zsh scripts/build_m6_signed_launch_agent_probe.sh'
    'zsh scripts/build_m6_signed_keychain_probe.sh' 'make bundle')
identifiers=('dev.samsungtvremote.local.m6agentprobe'
    'dev.samsungtvremote.local.m6keychainprobe' 'dev.samsungtvremote.local')
if (( keychain_only )); then
    names=("${names[@]:1}")
    rebuild_commands=("${rebuild_commands[@]:1}")
    identifiers=("${identifiers[@]:1}")
fi

for (( index = 1; index <= ${#bundles}; index++ )); do
    bundle="${bundles[$index]}"
    if [[ ! -d "$bundle" ]]; then
        print -ru2 -- "Missing bundle: $bundle"
        exit 2
    fi
    if ! codesign --verify --deep --strict "$bundle"; then
        print -ru2 -- "Cannot stage ${names[$index]}: the selected source bundle failed signature verification."
        if [[ ! -f "$bundle/Contents/_CodeSignature/CodeResources" ]]; then
            print -ru2 -- 'The bundle resource seal (Contents/_CodeSignature/CodeResources) is missing; this may be an unfinished build.'
        fi
        print -ru2 -- "In the usual desktop account, rebuild with: ${rebuild_commands[$index]}"
        print -ru2 -- 'Then rerun staging. No older build was substituted and no signatures were changed.'
        if [[ "${identifiers[$index]}" == 'dev.samsungtvremote.local' ]]; then
            print -ru2 -- 'Preserve a verified legacy GUI for the M6 experiment before rebuilding; use explicit bundle paths if needed.'
        fi
        exit 2
    fi
    identifier="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Info.plist")"
    if [[ "$identifier" != "${identifiers[$index]}" ]]; then
        print -ru2 -- "Unexpected bundle identifier for ${names[$index]}: $identifier"
        exit 2
    fi
done

stage="$(mktemp -d /private/tmp/m6probe-bundles.XXXXXX)"
staged=0
cleanup() {
    if (( ! staged )); then
        rm -rf -- "$stage"
    fi
}
trap cleanup EXIT

for (( index = 1; index <= ${#bundles}; index++ )); do
    ditto --noacl "${bundles[$index]}" "$stage/${names[$index]}"
    codesign --verify --deep --strict "$stage/${names[$index]}"
done

# The copied runbook must name this handoff, even after several preparations.
sed "s|^stage='/private/tmp/m6probe-bundles\.[^']*'$|stage='$stage'|" \
    "$repo_root/docs/phase-2/milestone-06-human-runbook.md" \
    > "$stage/milestone-06-human-runbook.md"
for reference in milestone-06-acceptance.md milestone-06-platform-evidence.md milestone-06-review.md; do
    cp "$repo_root/docs/phase-2/$reference" "$stage/$reference"
done
cat > "$stage/HANDOFF.txt" <<EOF
P2-M6 human test handoff
Prepared (UTC): $(date -u '+%Y-%m-%d %H:%M:%S')
Repository HEAD at staging (not the binary build revision): $(git -C "$repo_root" rev-parse --short HEAD)
Keychain-only handoff: $keychain_only (1 = optional LaunchAgent omitted)
These are existing signed bundles, not a rebuild from the current worktree.
No probe was run, Keychain item accessed, or LaunchAgent registered by staging.

Log in through the macOS login screen as m6probe, then follow
milestone-06-human-runbook.md in this directory.
For an existing installation, section 1 uninstalls the old test apps and
disposable state before reinstalling. Complete optional-agent and Keychain
cleanup before the file-removal block; run this reset only inside m6probe.
Its three directly linked context records are included alongside it; further
links within those records refer to the repository documentation.

stage='$stage'

This directory is temporary. If it disappears, prepare a fresh handoff from
the repository with zsh scripts/stage_m6_human_handoff.sh --keychain-only
for the required Keychain experiment. Omit --keychain-only to include the
optional LaunchAgent comparison.
EOF

# mktemp starts private. Publish only verified, non-secret artifacts, readable
# by the isolated account but writable only by the preparing user.
chmod -R a+rX,go-w "$stage"
staged=1
print -r -- "stage='$stage'"
print -r -- "Runbook: $stage/milestone-06-human-runbook.md"
