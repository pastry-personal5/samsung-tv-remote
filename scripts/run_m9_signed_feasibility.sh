#!/bin/zsh
# Disposable preliminary evidence, never production-profile cutover acceptance.
set -euo pipefail

repo_root="${0:A:h:h}"
cd "$repo_root"
if [[ -z "${SAMSUNG_TV_CODESIGN_IDENTITY:-}" && -s target/local-signing-identity.txt ]]; then
    IFS= read -r SAMSUNG_TV_CODESIGN_IDENTITY < target/local-signing-identity.txt
fi
if [[ -z "${SAMSUNG_TV_CODESIGN_IDENTITY:-}" ]]; then
    print -u2 'Configure an existing local signing identity before running this probe.'
    exit 2
fi

cargo build --example signed_peer_probe --example credential_probe
probe_dir="$repo_root/target/m9-feasibility"
mkdir -p "$probe_dir"
for fixture in signed_peer_probe credential_probe; do
    cp "target/debug/examples/$fixture" "$probe_dir/$fixture"
    codesign --force --sign "$SAMSUNG_TV_CODESIGN_IDENTITY" --timestamp=none \
        --identifier "dev.samsungtvremote.local.m9-$fixture" "$probe_dir/$fixture"
    codesign --verify --strict "$probe_dir/$fixture"
done

"$probe_dir/signed_peer_probe"
probe_account="$(uuidgen | tr '[:upper:]' '[:lower:]')"
probe_created=false
cleanup() {
    if [[ "$probe_created" == true ]]; then
        "$probe_dir/credential_probe" delete "$probe_account"
    fi
}
trap cleanup EXIT
probe_created=true
"$probe_dir/credential_probe" create "$probe_account"
"$probe_dir/credential_probe" read "$probe_account"
"$probe_dir/credential_probe" update "$probe_account"
"$probe_dir/credential_probe" read "$probe_account"
"$probe_dir/credential_probe" delete "$probe_account"
probe_created=false
print 'Preliminary signed feasibility passed; final bundle/platform gates remain open.'
