#!/bin/zsh
# Builds a signed, non-running migration probe for an isolated macOS account.
set -euo pipefail

repo_root="${0:A:h:h}"
identity_file="$repo_root/target/local-signing-identity.txt"
if [[ ! -f "$identity_file" ]]; then
    print -u2 'A local test signing identity is required; see the contribution guide.'
    exit 2
fi
identity="$(<"$identity_file")"
# Keep an unfinished build outside the staging script's discovery glob.
# A compile/signing failure must never shadow the last complete bundle.
mkdir -p "$repo_root/target"
probe_dir="$(mktemp -d "$repo_root/target/.m6-keychain.XXXXXX")"
published_dir="$repo_root/target/m6-keychain.${probe_dir##*.}"
published=0
cleanup() {
    if (( ! published )); then
        rm -rf -- "$probe_dir"
    fi
}
trap cleanup EXIT
bundle="$probe_dir/M6 Keychain Probe.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Helpers"
mkdir -p "$repo_root/target/m6-swift-module-cache"
swiftc -target arm64-apple-macos11 \
    -module-cache-path "$repo_root/target/m6-swift-module-cache" \
    "$repo_root/scripts/m6_keychain_probe.swift" \
    -o "$bundle/Contents/MacOS/m6-keychain-probe"
cp "$bundle/Contents/MacOS/m6-keychain-probe" \
    "$bundle/Contents/Helpers/m6-keychain-probe-helper"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.samsungtvremote.local.m6keychainprobe</string>
<key>CFBundleExecutable</key><string>m6-keychain-probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleName</key><string>M6 Keychain Probe</string>
</dict></plist>
PLIST
codesign --force --sign "$identity" --timestamp=none \
    "$bundle/Contents/Helpers/m6-keychain-probe-helper"
codesign --force --sign "$identity" --timestamp=none "$bundle"
codesign --verify --deep --strict "$bundle"
# Publish only a verified bundle; staging never sees the intermediate tree.
if [[ -e "$published_dir" ]]; then
    print -ru2 -- "Refusing to replace existing probe: $published_dir"
    exit 2
fi
mv -- "$probe_dir" "$published_dir"
published=1
print "Signed probe bundle (not run): $published_dir/${bundle:t}"
