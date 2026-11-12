#!/bin/zsh
set -euo pipefail

repo_root="${0:A:h:h}"
cd "$repo_root"
if (( $# != 1 )) || [[ -z "${SAMSUNG_TV_CODESIGN_IDENTITY:-}" ]]; then
    print -u2 'Usage: SAMSUNG_TV_CODESIGN_IDENTITY=<Apple identity> zsh scripts/build_m9_access_handoff.sh <candidate.app>'
    exit 2
fi
candidate="${1:A}"
/usr/bin/codesign --verify --strict '-R=anchor apple generic' "$candidate"
metadata=$(/usr/bin/codesign --display --verbose=4 -r- "$candidate" 2>&1)
requirement=""
for line in "${(@f)metadata}"; do
    if [[ "$line" == 'designated => '* ]]; then
        requirement="${line#designated => }"
    fi
done
if [[ -z "$requirement" ]]; then
    print -u2 'Candidate designated requirement missing.'
    exit 2
fi

cargo build --release --example authorize_api_access
handoff=$(mktemp -d "$repo_root/target/m9-access-handoff.XXXXXX")
helper="$handoff/authorize_api_access"
cp target/release/examples/authorize_api_access "$helper"
/usr/bin/codesign --force --sign "$SAMSUNG_TV_CODESIGN_IDENTITY" --timestamp=none \
    --identifier dev.samsungtvremote.local --requirements "=designated => $requirement" "$helper"
/usr/bin/codesign --verify --strict "$helper"
# Inspect verifies matching actual Apple signatures and performs no Keychain API calls.
"$helper" --inspect "$candidate"
cat > "$handoff/README.txt" <<'TEXT'
This standalone foreground helper is not part of app startup.
Inspection has made no Keychain calls. No signer setting was changed.

After explicit user approval, run this helper with --request-access and the
same signed candidate.app path. macOS may request permission for each of the
existing gui/mcp items in dev.samsungtvremote.local.control-v1.api.
The user must review and respond to those native dialogs. Persistent access
requires their explicit native approval. Cancellation stops further requests.

The helper only reads those two existing items in the default keychain and
reports statuses. It never creates, updates, deletes or prints their values.
It does not unlock a locked keychain, change trust, or access TV/legacy items.
It does not start/stop the app or server. Keep the candidate unchanged.
TEXT
print "Prepared inspection-only handoff: $handoff"
