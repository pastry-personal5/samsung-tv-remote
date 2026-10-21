#!/bin/zsh
# Builds but never registers a signed app-bundled LaunchAgent test candidate.
set -euo pipefail

repo_root="${0:A:h:h}"
identity_file="$repo_root/target/local-signing-identity.txt"
if [[ ! -f "$identity_file" ]]; then
    print -u2 'A local test signing identity is required; see the contribution guide.'
    exit 2
fi
identity="$(<"$identity_file")"
probe_dir="$(mktemp -d "$repo_root/target/m6-launch-agent.XXXXXX")"
bundle="$probe_dir/M6 LaunchAgent Probe.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" \
    "$bundle/Contents/Library/LaunchAgents" \
    "$repo_root/target/m6-swift-module-cache"

rustc --edition=2021 "$repo_root/scripts/m6_launch_agent_probe.rs" \
    -o "$bundle/Contents/Resources/m6-agent-helper"
swiftc -target arm64-apple-macos13 \
    -module-cache-path "$repo_root/target/m6-swift-module-cache" \
    "$repo_root/scripts/m6_launch_agent_controller.swift" \
    -o "$bundle/Contents/MacOS/m6-agent-controller"

cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.samsungtvremote.local.m6agentprobe</string>
<key>CFBundleExecutable</key><string>m6-agent-controller</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleName</key><string>M6 LaunchAgent Probe</string>
</dict></plist>
PLIST
cat > "$bundle/Contents/Library/LaunchAgents/dev.samsungtvremote.local.m6agent.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>dev.samsungtvremote.local.m6agent</string>
<key>BundleProgram</key><string>Contents/Resources/m6-agent-helper</string>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><false/>
</dict></plist>
PLIST
plutil -lint "$bundle/Contents/Info.plist" \
    "$bundle/Contents/Library/LaunchAgents/dev.samsungtvremote.local.m6agent.plist"
codesign --force --sign "$identity" --timestamp=none \
    "$bundle/Contents/Resources/m6-agent-helper"
codesign --force --sign "$identity" --timestamp=none "$bundle"
codesign --verify --deep --strict "$bundle"
print "Signed LaunchAgent comparison bundle (not registered): $bundle"
