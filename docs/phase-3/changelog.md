# Phase 3 decision log

Status: Active

- 2026-11-14 — Revised the plan. Split capability evidence from request
  outcomes and reused Phase 2's `not_sent`/`written`/`uncertain` set. Made
  source ports user-configured, because no probe can prove a source key.
  Added a target-TV frame spike to P3-M1, no-replay and untrusted-TV-data
  rules, server-owned IME sessions, per-TV storage covered by Forget
  recovery, config revisions on setting changes, and the MCP budget's lack
  of headroom. Recorded open decisions, including MCP text input and
  text-safe request fingerprints.

- 2026-11-14 — Planned Phase 3 from the archived Phase 2 proposal. Old P2-M1
  to P2-M5 stay Dropped; their scope returns as P3-M1 to P3-M5, adapted so the
  Control API Server owns probes, caches, and requests for both GUI and MCP.
  The phase starts after Phase 2 is Done.
