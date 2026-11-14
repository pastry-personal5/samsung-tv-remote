# Contribution Guide

Samsung TV Remote is a native macOS application written in Rust. This guide
defines the canonical local Cargo workflow for the current Rust package.

## Setup and development

Use stable Rust and Python 3. Build and open the debug GUI with:

```sh
cargo run
```

The GUI uses the Control API Server. Production startup requires a locally
signed executable; an unsigned/ad-hoc `cargo run` displays a signing block
without opening the production profile. Use the isolated fake probe below for
unsigned development. Build the signed app bundle with `make bundle`. Set
`SAMSUNG_TV_CODESIGN_IDENTITY` to an Apple Development or Developer ID identity,
or save that identity's fingerprint in `target/local-signing-identity.txt`.
The builder requires an Apple-backed signature and a TeamIdentifier before
replacing the existing bundle. The self-signed identity produced by
`scripts/create_local_signing_identity.sh` is for isolated probes and cannot
build a production bundle. The bundle is written to
`target/bundle/Samsung TV Remote.app`. Launch that bundle for native review;
`cargo run` does not prove native bundle permission behavior.

### Install and update the macOS app

Use one build-and-install command, then open the installed app:

```sh
make install-mac
make run-mac
make server-status
```

`install-mac` builds once, copies the signed bundle into a temporary directory
under `~/Applications`, and verifies it before closing the installed GUI and
server. The installer identifies processes by their exact executable path,
owner and start time, sends SIGTERM, and waits up to 30 seconds for them to exit.
This also recovers a detached server whose executable was overwritten by an
earlier installation. Shutdown affects every client attached to that server;
finish current TV operations and save GUI drafts before installing. If a
process does not settle, installation aborts without replacing its bundle or
force-killing it. Process-discovery errors also abort installation.

After settlement, the installer moves the old bundle aside, publishes the new
bundle by rename, and verifies its signature again. A publication/verification
failure restores the previous bundle; if rollback fails, both artifacts are
retained with their paths printed. Successful updates retain the previous app
in a printed `.samsung-tv-remote.previous-*.noindex` directory under the same
Applications directory. Only the application bundle is replaced. Profiles,
pairing/API credentials and intentional server Stop markers are preserved.
Concurrent installers are rejected. Avoid launching the app during replacement.

M11 bundle entrypoints hold a shared executable lock for their process lifetime.
The installer exclusively locks both old and staged executable inodes through
publication, verification and rollback. A concurrent relaunch either aborts
installation before replacement or exits before opening a profile; retry after
installation completes. Legacy builds do not take this startup lock; process
checks after the old bundle is moved aside catch observed late legacy launches
and restore it. Signed old/new upgrade acceptance remains open.

To roll back, pass the printed retained `.app` as the source to
`python3 scripts/install_macos_bundle.py install SOURCE DESTINATION`, with both
paths absolute. This verifies and settles processes just like an upgrade.
It preserves the current profile, credentials, Stop and enrollment; it is not
a data rollback. Each install retains another previous bundle. Keep the newest
recovery bundle and manually remove older retained bundles after review.

### Removal and recovery

In Settings → Servers, first **Disconnect Claude Code**, then **Stop** and
wait for owner settlement. Then move only the application bundle to Trash.
Disconnect durably disables MCP before removing the unchanged managed entry;
an externally changed entry reports a conflict while leaving access disabled.
Recheck ambiguous progress and preserve changed settings for explicit repair.
Disconnect also works while the owner is already stopped.

Deleting the bundle alone does not stop an already detached owner. Removal
keeps TV records, trust, Keychain credentials, intentional Stop and disabled MCP
enrollment. Reinstalling the bundle does not clear those controls. To restore
service explicitly **Resume**, **Connect Claude Code**, then **Verify**.
Endpoint verification is separate from observing Claude attachment. Full data
and credential erasure is outside this removal sequence. The hands-on signed
removal check is still pending; see the
[N11 runbook](phase-2/milestone-11-native-runbook.md#hands-on-removal-and-recovery--n11).

### Connection troubleshooting

- **Stopped:** explicitly Resume. Ordinary reopen, Verify and header refresh
  preserve an intentional Stop. A durable Stop acknowledgment and observed
  owner settlement are separate facts.
- **Crashed/unavailable:** reconnect from a fresh authenticated snapshot. An
  interrupted or expired operation can be unknown; do not replay it because
  its receipt is missing. Any new TV action needs explicit intent/current guards.
- **Incompatible build:** use the explicit restart action after reviewing its
  all-client effect; reopening cannot replace an incompatible owner.
- **Credentials blocked:** denied, locked and interaction-required states need
  their displayed explicit recovery. Routine retries never request native
  access dialogs. Follow the access-review procedure below when applicable.
- **Local Network unavailable:** review macOS permission and network access,
  then explicitly reconnect/re-probe. A successful loopback endpoint probe does
  not establish permission to reach the TV or prove the panel responded.
- **MCP saved but unverified:** Check, enrollment, saved entry, endpoint Verify
  and actual Claude attachment are separate facts. A relocated helper needs
  explicit repair/Connect; reinstall alone cannot restore a disabled grant.

If you already ran `make bundle`, install that exact signed artifact without
building/signing it again:

```sh
make install-built-bundle
make run-mac
```

`run-mac`/`run-mac-app` and the compatibility aliases `open-mac`/`open-mac-app`
open the installed bundle with normal macOS instance reuse. They do not rebuild,
install, reset credentials or forcibly restart a server. `server-status` checks
the installed executable's authenticated attachment without starting a server.
An intentional Stop still requires explicit Resume in Settings → Servers.

If native API access needs approval after a signer change, run:

```sh
make request-api-access
```

Review the native dialogs, then explicitly recheck credentials in Settings →
Servers. This separate foreground command reads existing API items and does
not start the server or create missing items; a first launch provisions them.
A successful request does not prove that an older running server's signature
is valid. In the observed failed upgrade, `codesign --verify --strict <server-pid>`
reported “the code on disk does not match what is running,” while the installed
bundle passed verification. The safe installer settles that old process before
replacement. Reopening another GUI cannot settle a detached server.

### Preserved candidate builds and signing review

For an update check while an older bundle is running, use `make bundle-candidate`.
It creates a new `target/.m9-candidate.XXXXXX.noindex/Samsung TV Remote.app` and preserves
the older executable. `make bundle` refuses to replace its ordinary output
bundle while an app/server is running from that exact path. Installation settles
only processes from the destination bundle; a server running from a different
candidate remains under its own explicit lifecycle controls.
The M9 controlled comparison found that the local self-signed identity could
not read its predecessor's Keychain items after a rebuild, despite an equal
designated requirement. Apple Development builds with the same TeamIdentifier
passed disposable cross-build read/update/delete. See the
[implementation evidence](phase-2/milestone-09-implementation.md#signed-rebuild-investigation-2026-11-12).
Existing self-signed items still require explicit access approval before a
signer change; do not delete or regenerate them to conceal an access failure.
M9 native acceptance remains open. The standalone foreground helper can read
the existing API items after native approval while the app still reports
access denied; approval of a different helper path is insufficient evidence.

If an installed GUI reports `bootstrap_denied` while an approved candidate's
`--server-status` reports `serving/ready`, compare their
`codesign -d --verbose=4 -r-` output. A self-signed installed GUI cannot
authenticate to an Apple-signed owner. Install the approved Apple-signed bundle and correct the saved build
identity; credential recheck cannot repair a signing identity mismatch. Keep
existing API credentials and TV data intact.

For explicit access review, use the actual Apple-signed candidate executable:

```sh
'/absolute/path/to/Samsung TV Remote.app/Contents/MacOS/samsung-tv-remote' \
  --server-api-access --inspect
'/absolute/path/to/Samsung TV Remote.app/Contents/MacOS/samsung-tv-remote' \
  --server-api-access --request-access
```

Inspection verifies the Apple-backed signature without Keychain calls. Run
request mode only with explicit approval and foreground review of native
dialogs. It reads only the existing fresh-service `gui`/`mcp` API items, never
creates/replaces/deletes values, and stops after a blocked first item. It does
not open profiles or start a server. Routine startup and credential recheck
never invoke this mode. Verify UI-off access separately afterward; a successful
foreground read alone is not persistent-access evidence.

The standalone API access helper can still be prepared for inspection with:

```sh
SAMSUNG_TV_CODESIGN_IDENTITY='<Apple Development identity>' \
  zsh scripts/build_m9_access_handoff.sh '/absolute/path/to/Samsung TV Remote.app'
```

This creates `target/m9-access-handoff.XXXXXX` with a README and signed helper.
The builder only runs `--inspect`: it verifies matching Apple-backed signatures
and makes no Keychain calls. It never changes the saved signer. The separate
`--request-access` mode requires explicit user approval and foreground review
of native dialogs for the existing fresh-service `gui`/`mcp` API items. It reads
those items without creating, replacing, deleting or printing their values;
it stops after a blocked first item. It is not part of application startup or
credential recheck. Do not use it for legacy or TV credentials.

Run individual tests while iterating with `cargo test <name>`. Do not require a
physical TV or a local network for automated tests; use deterministic fakes for
protocol and pairing boundaries.

M8 HTTP integration tests use an ephemeral literal loopback listener and fake
TV/credential ports:

```sh
cargo test --test control_http
```

They require permission to bind `127.0.0.1` in a sandboxed environment; they
contact no TV or LAN host. Process fixtures also spawn the same executable;
the descriptor/session fixture compiles a small C child with the system `cc`.

Build and exercise the isolated M8 server using an existing absolute parent:

```sh
cargo build
m8_profile=$(mktemp -d /private/tmp/samsung-remote-m8.XXXXXX)
target/debug/samsung-tv-remote --server-start --fake-profile "$m8_profile"
target/debug/samsung-tv-remote --server-status --fake-profile "$m8_profile"
target/debug/samsung-tv-remote --gui-probe --fake-profile "$m8_profile"
```

The probe uses the existing GUI through HTTP/SSE and constructs no local TV
runtime. In Settings, explicitly probe `synthetic-tv.invalid`, confirm its
synthetic fingerprint and Pair; remote actions then use the fake session.
Closing the probe leaves the server running. Reopen it to attach again.
The default signed GUI uses the same client workflows with the production
server. It owns no local TV runtime; closing it detaches without stopping the
server. Production storage is fresh and separate from legacy data. Never use
the development namespace with production storage or Keychain ports.

Server status and management controls are in Settings → Servers for both the
production GUI and development probe.
Stop requires confirmation that it affects every client and keeps the GUI open.
Its result distinguishes a durable acknowledgment from a marker observed
after a lost reply, and reports settlement separately. Resume is explicit.
An incompatible server offers explicit restart. Credential/state recheck makes
one protected attachment attempt; it does not start a missing server, clear
Stop or restore revoked access. TV mutations stay disabled during management
and until a fresh authenticated snapshot arrives. Local Network and signed
production acceptance still require the final bundle checks in M9.

Management commands (all require the same `--fake-profile` argument):

```sh
target/debug/samsung-tv-remote --server-stop --fake-profile "$m8_profile"
target/debug/samsung-tv-remote --server-resume --fake-profile "$m8_profile"
target/debug/samsung-tv-remote --server-restart --fake-profile "$m8_profile"
```

Stop persists before its durable acknowledgment. A lost reply may report
`marker_observed` with `durable_acknowledgment:false`; that observation does not
prove drain finished. Definite persistence failures remain errors. Ordinary
start/reopen remains stopped until explicit GUI Resume. An incompatible live build requires explicit restart;
occupied ports fail without reallocating. `--server` runs the same fake
composition in the foreground; no mode starts at login. Management commands
never print credentials. The separate M10 `--mcp-headers` mode is intentionally
limited to one MCP Authorization JSON object for Claude's dynamic header pipe;
never redirect that output to a file or include it in diagnostics. A profile added through fake Pairing survives restart
with synthetic presence markers in its distinct credential directory.

The debug GUI probe supports renderer-only capture without desktop access:
set `SAMSUNG_TV_REMOTE_PROBE_CAPTURE` to an absent absolute `.ppm` output path
when launching `--gui-probe`; after eight seconds it writes this window's
pixels once. Convert with `sips -s format png INPUT.ppm --out OUTPUT.png`.
The hook is inactive in release builds and in the default GUI. The checked-in
[M8 capture](phase-2/evidence/m8-gui-probe-connected.png) was produced this way.
To capture the Servers Settings panel, also set
`SAMSUNG_TV_REMOTE_PROBE_CAPTURE_SETTINGS=1`; the debug probe opens that panel
and captures its window rather than the main window. Connection actions in a
development probe require an explicitly isolated `CLAUDE_CONFIG_DIR`.
`cargo test --test server_process` verifies start/status/Stop/Resume/restart,
fresh-profile persistence, GUI-client outage recovery and detached lifetime;
`cargo test --test control_http_heap -- --nocapture` measures full-profile
HTTP/SSE heap overhead. These are fake-port checks, not signed bundle or real
local-network permission evidence.

## Required validation

Before opening a pull request, run the full checks from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`cargo test` also runs `scripts/check_gui_boundary.py --self-test`, enforcing
that presentation and default composition cannot import runtime/TV/storage
adapters, server handles or raw service receipts. Run it directly with Python 3
while changing the boundary. Its negative corpus checks that forbidden cases
fail and that the fake-only debug renderer capture stays narrowly scoped.

`cargo test --test bundle_install --test bundle_signing` covers installation
ordering, process identity, settlement timeout, signature failure, retained
rollback artifacts and the running-build guard with temporary bundles and fake
process/signing boundaries. Run its Python cases directly with
`python3 -B tests/test_macos_bundle_install.py`. These tests never stop the
installed app, access Keychain or replace the owner's bundle.

Format with `cargo fmt` when the formatting check fails. Address Clippy
warnings rather than suppressing them without a documented reason. `cargo test`
runs the package test suite; add or update tests for each behavior change and
bug fix.

## Commit messages

Write commit messages in [Conventional Commits](https://www.conventionalcommits.org)
(Angular) style.

- **Title:** `<type>[(scope)]: <summary>` — lowercase, imperative, 50 characters
  or fewer, no trailing period. Types: `feat`, `fix`, `docs`, `style`,
  `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- **Body** (optional): one blank line after the title, then short paragraphs
  explaining *why* the change was made, wrapped at 72 characters. Keep it brief.

Keep each commit focused. Example:

    feat(discovery): add SSDP TV discovery

    Find TVs on the local network so the remote can list devices without manual
    IP entry.

## Pull requests

Keep a pull request focused and explain the user-visible effect, relevant
protocol or UI decisions, and every validation command run. Include a
screenshot for macOS UI changes and sanitized logs for connection or pairing
changes. Never commit TV pairing tokens, device IP addresses, credentials, or
other local-network details.
