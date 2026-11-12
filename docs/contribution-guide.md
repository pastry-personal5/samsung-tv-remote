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
unsigned development. Build the signed app bundle with `make bundle`. It uses `SAMSUNG_TV_CODESIGN_IDENTITY` if set, or the
identity saved in `target/local-signing-identity.txt` by
`zsh scripts/create_local_signing_identity.sh`. The bundle is written to
`target/bundle/Samsung TV Remote.app`. Launch that bundle for native review;
`cargo run` does not prove native bundle permission behavior.

For an update check while an older bundle is running, use `make bundle-candidate`.
It creates a new `target/m9-candidate.XXXXXX/Samsung TV Remote.app` and preserves
the older executable. `make bundle` overwrites the ordinary bundle path.
The M9 controlled comparison found that the local self-signed identity could
not read its predecessor's Keychain items after a rebuild, despite an equal
designated requirement. Apple Development builds with the same TeamIdentifier
passed disposable cross-build read/update/delete. See the
[implementation evidence](phase-2/milestone-09-implementation.md#signed-rebuild-investigation-2026-11-12).
Existing self-signed items still require explicit access approval before a
signer change; do not delete or regenerate them to conceal an access failure.
M9 native acceptance remains open.

The explicit foreground API access helper can be prepared for inspection with:

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

The probe's server status bar is shared by the main and Settings windows.
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
composition in the foreground; no mode starts at login. Credentials never
appear in command output. A profile added through fake Pairing survives restart
with synthetic presence markers in its distinct credential directory.

The debug GUI probe supports renderer-only capture without desktop access:
set `SAMSUNG_TV_REMOTE_PROBE_CAPTURE` to an absent absolute `.ppm` output path
when launching `--gui-probe`; after eight seconds it writes this window's
pixels once. Convert with `sips -s format png INPUT.ppm --out OUTPUT.png`.
The hook is inactive in release builds and in the default GUI. The checked-in
[M8 capture](phase-2/evidence/m8-gui-probe-connected.png) was produced this way.
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
