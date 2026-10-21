#!/bin/zsh
# Exercises isolated signed process survival, sticky Stop, and port collision.
set -euo pipefail
unsetopt BG_NICE

repo_root="${0:A:h:h}"
identity_file="$repo_root/target/local-signing-identity.txt"
if [[ ! -f "$identity_file" ]]; then
    print -u2 'A local test signing identity is required; see the contribution guide.'
    exit 2
fi
identity="$(<"$identity_file")"
probe_dir="$(mktemp -d "$repo_root/target/m6-lifecycle.XXXXXX")"
bundle="$probe_dir/M6 Lifecycle.app"
state_dir="$probe_dir/state"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Helpers"
mkdir -m 700 "$state_dir"
rustc --edition=2021 "$repo_root/scripts/m6_signed_lifecycle_probe.rs" -o "$bundle/Contents/MacOS/probe"
cp "$bundle/Contents/MacOS/probe" "$bundle/Contents/Helpers/probe-helper"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.samsungtvremote.local.m6probe</string>
<key>CFBundleExecutable</key><string>probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleName</key><string>M6 Lifecycle Probe</string>
</dict></plist>
PLIST
codesign --force --sign "$identity" --timestamp=none "$bundle/Contents/Helpers/probe-helper"
codesign --force --sign "$identity" --timestamp=none "$bundle"
codesign --verify --deep --strict "$bundle"

port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
server_pid=''
unrelated_pid=''
cleanup() {
    if [[ -n "$server_pid" ]]; then
        kill "$server_pid" 2>/dev/null || true
    fi
    if [[ -n "$unrelated_pid" ]]; then
        kill "$unrelated_pid" 2>/dev/null || true
    fi
}
trap cleanup EXIT

for server_path in "$bundle/Contents/MacOS/probe" "$bundle/Contents/Helpers/probe-helper"; do
    server_pid="$("$bundle/Contents/MacOS/probe" --launch "$port" "$server_path" "$state_dir")"
    ready=''
    for attempt in {1..30}; do
        ready="$(curl --max-time 1 -fsS "http://127.0.0.1:$port/" 2>/dev/null || true)"
        [[ "$ready" == 'm6-ready' ]] && break
        sleep 0.1
    done
    [[ "$ready" == 'm6-ready' ]] || { print -u2 'Server did not survive launcher exit'; exit 1; }
    if "$server_path" --serve "$port" "$state_dir" > /dev/null 2>&1; then
        print -u2 'Port collision was accepted'; exit 1
    fi
    print "PASS signed detached server survived launcher exit and rejected collision: ${server_path:t}"
    if [[ "$server_path" == "$bundle/Contents/MacOS/probe" ]]; then
        stopped="$(curl --max-time 2 -fsS -X POST "http://127.0.0.1:$port/stop")"
        [[ "$stopped" == 'm6-stopped' ]] || { print -u2 'Stop was not acknowledged'; exit 1; }
        server_pid=''
        [[ -f "$state_dir/stopped" && "$(stat -f %Lp "$state_dir/stopped")" == '600' ]] || {
            print -u2 'Owner-only Stop marker was not durable'; exit 1
        }
        if "$bundle/Contents/MacOS/probe" --launch "$port" "$server_path" "$state_dir" > /dev/null 2>&1; then
            print -u2 'Launcher reversed explicit Stop'; exit 1
        fi
        server_pid="$("$bundle/Contents/MacOS/probe" --resume "$port" "$server_path" "$state_dir")"
        [[ ! -e "$state_dir/stopped" ]] || { print -u2 'Resume left Stop marker'; exit 1; }
        ready=''
        for attempt in {1..30}; do
            ready="$(curl --max-time 1 -fsS "http://127.0.0.1:$port/" 2>/dev/null || true)"
            [[ "$ready" == 'm6-ready' ]] && break
            sleep 0.1
        done
        [[ "$ready" == 'm6-ready' ]] || { print -u2 'Explicit Resume failed'; exit 1; }
        print 'PASS signed launcher respected sticky Stop and explicit Resume'
    fi
    if [[ "$server_path" == "$bundle/Contents/MacOS/probe" ]]; then
        kill -9 "$server_pid"
        for attempt in {1..30}; do
            curl --max-time 1 -fsS "http://127.0.0.1:$port/" > /dev/null 2>&1 || break
            sleep 0.1
        done
        [[ ! -e "$state_dir/stopped" ]] || { print -u2 'Crash created Stop marker'; exit 1; }
        print 'PASS forced crash left no sticky Stop marker for next launch'
    else
        kill "$server_pid"
    fi
    wait "$server_pid" 2>/dev/null || true
    server_pid=''
done
python3 -m http.server --bind 127.0.0.1 "$port" > /dev/null 2>&1 &
unrelated_pid="$!"
for attempt in {1..30}; do
    curl --max-time 1 -fsS "http://127.0.0.1:$port/" > /dev/null 2>&1 && break
    sleep 0.1
done
if "$bundle/Contents/MacOS/probe" --serve "$port" "$state_dir" > /dev/null 2>&1; then
    print -u2 'Unrelated listener collision was accepted'; exit 1
fi
print 'PASS signed server rejected a port held by an unrelated listener'
print "Isolated probe bundle: $bundle"
