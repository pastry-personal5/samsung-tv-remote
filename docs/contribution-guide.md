# Contribution Guide

Samsung TV Remote is a native macOS application written in Rust. This guide
defines the canonical local Cargo workflow for the current Rust package.

## Setup and development

Use stable Rust. Build and launch the debug application with:

```sh
cargo run
```

The package launches the Iced GUI. Settings can probe and pair a TV over
secure port 8002. For local-network permission testing, build the signed app
bundle with `make bundle`. It uses `SAMSUNG_TV_CODESIGN_IDENTITY` if set, or the
identity saved in `target/local-signing-identity.txt` by
`zsh scripts/create_local_signing_identity.sh`. The bundle is written to
`target/bundle/Samsung TV Remote.app`. Launch that bundle for native review;
`cargo run` does not prove native bundle permission behavior.

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
The default `cargo run` GUI keeps its existing in-process composition until M9.
Never use the development namespace with production storage or Keychain ports.

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
