# Samsung TV Remote

> Super-fast remote controller for macOS, written in Rust.

[![Release](https://img.shields.io/github/v/release/pastry-personal5/samsung-tv-remote?label=release)](https://github.com/pastry-personal5/samsung-tv-remote/releases/latest)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen)](docs/contribution-guide.md)

Save time controlling your Samsung TV from your Mac. It is built for the
moments when powering on your own Samsung TV through the official SmartThings
app feels slower than reaching for a remote.

The app supports discovery or manual address entry, certificate checking,
TV-approved pairing, saved-TV reconnect, remote keys, and Wake-on-LAN. Power,
Remote, and Settings are functional.

## Why it's super-fast

Many remote apps feel sluggish: a slow launch, a lag after every tap, a
button press that takes a moment to reach the TV. Samsung TV Remote is built
to remove that delay.

- **100% Rust.** One native codebase, from the UI to the Samsung protocol,
  with no interpreter or bridge layer between your click and the TV.
- **No Electron, no web runtime.** The app opens instantly instead of loading
  a browser engine first.
- **No garbage collector.** No pauses, so every key press has predictable,
  low latency.
- **Direct to your TV.** Commands travel straight over your local network,
  with no cloud round trip and no waiting on a remote server.
- **Always ready.** Saved-TV reconnect and Wake-on-LAN get you from "I want to
  watch" to a powered-on TV faster than reaching for a phone app.

Release: [v1.0.0](docs/releases/v1.0.0.md).

![img](docs/archive/screenshots/v1.0.0-0000.jpg) ![img](docs/archive/screenshots/v1.0.0-0001.jpg)

See the [documentation index](docs/README.md) for current guidance
and archived plans.

For setup, development, and validation, use the
[contribution guide](docs/contribution-guide.md). Do not commit pairing tokens,
TV addresses, or local-network captures.
