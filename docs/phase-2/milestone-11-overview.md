# P2-M11: Integration and Refactor Completion

Status: Planned

## Goal

Finish the server boundary, harden the signed two-process macOS release first
introduced in P2-M9, and verify the GUI and MCP surfaces together. Remove any
remaining direct-control paths.

## Scope

- Harden installation, upgrade, explicit start and stop, crash recovery, and
  uninstall for the signed per-user server and GUI; document the local
  endpoint and client credential setup without enabling login-time launch.
- Verify two clients acting at once, a GUI exit while MCP continues, server
  restart, broken event streams, selection replacement, Pairing conflicts,
  queue pressure, uncertain writes, and saved-data migration.
- Review native accessibility and target-TV behavior, then update current
  architecture, contribution, user, and troubleshooting documentation.

## Completion checklist

- [ ] The P2-M9 signed bundle passes cold launch, upgrade, explicit stop,
  relaunch, sleep/wake, and uninstall checks. The GUI reconnects to a
  compatible existing server; an incompatible old one drains before
  replacement. Closing the GUI leaves MCP available. A configured Claude
  Code connection can start an absent server, but neither client undoes an
  intentional Stop. A second instance cannot race the first, an unrelated
  listener fails closed,
  and fast user switching cannot attach to another user's server. No
  login-time launch is added.
- [ ] GUI and MCP clients get consistent state and terminal outcomes under
  concurrent requests, atomic subscription, slow observers, stream gaps,
  selection replacement, network interruption, and restart. A changed
  server epoch rejects stale mutations; an expired or crash-lost outcome is
  reported unknown, never retried automatically. Same-key deduplication is
  guaranteed only within the documented retention window.
- [ ] MCP catalog and workflow token budgets set in M6 remain within their
  recorded thresholds under the installed Claude Code target, including an
  all-tools-upfront fallback. Report token estimates and tool-call counts for
  routine remote control, setup, and stale/unknown recovery without weakening
  safety checks or hiding uncertainty.
- [ ] The legacy in-process control adapter and direct GUI access to Samsung,
  SSDP, Wake, Keychain, and preferences are removed; the dependency boundary
  is checked in tests or build configuration.
- [ ] The final GUI keeps local presentation state only: views do not call the
  HTTP client directly, view models do not hold service/persistence policy,
  and UI messages or tasks do not carry TV sessions or Keychain objects.
  Command-generation and projection tests pass with a fake client.
- [ ] Existing records and Keychain items survive upgrade, and the documented
  rollback procedure leaves recoverable data. Test/development credentials
  and Keychain items remain isolated from the production profile. Secrets and
  TV addresses are absent from logs and test artifacts.
- [ ] No routine terminal command, manual port selection, or token copy is
  needed. Signed GUI and MCP workflows show zero routine Keychain prompts
  across start, close/reopen, reconnect, and upgrade. A denied, locked, or
  interaction-required item fails visibly without an unattended prompt or
  retry loop. Tests record Gatekeeper, Local Network, exceptional legacy
  Keychain ACL/unlock, TV Pairing, and optional MCP client consent separately
  from avoidable repeated prompts. The release artifact's signing/notarization
  policy records the owner’s local signed installation target. Notarization
  and public distribution are outside Phase 2; a local signature is not
  evidence of public distribution acceptance.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo test` pass; signed-bundle, native accessibility, MCP client, and
  sanitized target-TV checks are recorded with any deferred evidence. Native
  review includes the server process's Local Network permission and Keychain
  access under the signed bundle.

## References

- [M6 acceptance and test ownership](milestone-06-acceptance.md)

- [Architecture](milestone-11-architecture.md)
- [Target architecture](target-architecture.md)
- [Lifecycle and security research](server-lifecycle-security.md)
- [Control API Server internals](control-server-internals.md)
