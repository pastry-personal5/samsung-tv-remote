#!/bin/zsh
set -euo pipefail

repo_root="${0:A:h:h}"
cd "$repo_root"

if (( $# > 1 )); then
    print -u2 'Expected at most one bundle option.'
    exit 2
fi
case "${1:-}" in
    ''|--unsigned-preview) ;;
    --acceptance-candidate|--p3-spike-candidate) ;;
    *) print -u2 'Unknown bundle option.'; exit 2 ;;
esac

if [[ "${1:-}" != "--unsigned-preview" && -z "${SAMSUNG_TV_CODESIGN_IDENTITY:-}" ]]; then
    print -u2 'Set SAMSUNG_TV_CODESIGN_IDENTITY to a local code-signing identity.'
    print -u2 'Use --unsigned-preview only to inspect bundle structure and metadata.'
    exit 2
fi

# A running build-path server must retain its executable for peer validation.
# Candidate builds always use a new path and need no shutdown.
if [[ "${1:-}" != "--acceptance-candidate" && "${1:-}" != "--p3-spike-candidate" ]]; then
    python3 scripts/install_macos_bundle.py check-idle "$repo_root/target/bundle/Samsung TV Remote.app"
fi

if [[ "${1:-}" == "--p3-spike-candidate" ]]; then
    cargo build --release --features p3-spike
else
    cargo build --release
fi

# Validate a new artifact before touching the ordinary bundle. A saved local
# self-signed identity must not replace an app authorized by an Apple server.
staging=$(mktemp -d "$repo_root/target/bundle-staging.XXXXXX")
preserve_staging=false
cleanup_staging() {
    if [[ "$preserve_staging" == false ]]; then
        rm -rf "$staging"
    else
        print -u2 "Rollback files retained at $staging"
    fi
}
trap cleanup_staging EXIT
staged_bundle="$staging/Samsung TV Remote.app"
if [[ "${1:-}" == "--acceptance-candidate" || "${1:-}" == "--p3-spike-candidate" ]]; then
    # Preserve a live older bundle for signed update/compatibility checks.
    # Each candidate is new; no running executable is overwritten in place.
    # Keep review-only candidates out of macOS application discovery. LaunchServices
    # otherwise registers every retained candidate as another installed app.
    candidate_parent=$(mktemp -d "$repo_root/target/.m9-candidate.XXXXXX")
    noindex_candidate_parent="${candidate_parent}.noindex"
    mv "$candidate_parent" "$noindex_candidate_parent"
    candidate_parent="$noindex_candidate_parent"
    bundle="$candidate_parent/Samsung TV Remote.app"
else
    bundle="$repo_root/target/bundle/Samsung TV Remote.app"
fi
contents="$staged_bundle/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"
cp "$repo_root/target/release/samsung-tv-remote" "$contents/MacOS/samsung-tv-remote"

cat > "$contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleDisplayName</key><string>Samsung TV Remote</string>
    <key>CFBundleExecutable</key><string>samsung-tv-remote</string>
    <key>CFBundleIdentifier</key><string>dev.samsungtvremote.local</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>Samsung TV Remote</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>2.0.0</string>
    <key>CFBundleVersion</key><string>3</string>
    <key>NSLocalNetworkUsageDescription</key>
    <string>Connect to a TV on your local network for pairing, waking, and remote control.</string>
</dict>
</plist>
PLIST

if [[ "${1:-}" == "--p3-spike-candidate" ]]; then
    python3 - "$contents/Resources/p3-spike-build.json" <<'PYMETA'
import json
import subprocess
import sys
from pathlib import Path
commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], text=True))
if len(commit) not in (40, 64) or any(c not in "0123456789abcdef" for c in commit):
    raise SystemExit("Invalid build commit identity")
Path(sys.argv[1]).write_text(json.dumps({"commit": commit, "dirty": dirty}) + "\n")
PYMETA
fi

plutil -lint "$contents/Info.plist"
if [[ "${1:-}" != "--unsigned-preview" ]]; then
    codesign --force --sign "$SAMSUNG_TV_CODESIGN_IDENTITY" --timestamp=none "$staged_bundle"
    if ! codesign --verify --strict '-R=anchor apple generic' "$staged_bundle"; then
        print -u2 'Production bundles require an Apple Development or Developer ID signing identity.'
        print -u2 'The configured signer cannot provide stable Keychain access across builds; existing bundle preserved.'
        exit 1
    fi
    metadata=$(codesign --display --verbose=4 "$staged_bundle" 2>&1)
    if ! print -r -- "$metadata" | /usr/bin/grep -Eq '^TeamIdentifier=[A-Za-z0-9]{10}$'; then
        print -u2 'Production bundles require an Apple signing TeamIdentifier; existing bundle preserved.'
        exit 1
    fi
fi
# Move the old bundle aside only after signing and verification have succeeded.
mkdir -p "${bundle:h}"
python3 scripts/install_macos_bundle.py check-idle "$bundle"
if [[ -e "$bundle" ]]; then
    mv "$bundle" "$staging/previous.app"
fi
if ! mv "$staged_bundle" "$bundle"; then
    if [[ -e "$staging/previous.app" ]]; then
        if ! mv "$staging/previous.app" "$bundle"; then
            preserve_staging=true
        fi
    fi
    exit 1
fi
if [[ "${1:-}" == "--unsigned-preview" ]]; then
    print "Unsigned bundle preview created at $bundle"
else
    print "Locally signed bundle created at $bundle"
fi
