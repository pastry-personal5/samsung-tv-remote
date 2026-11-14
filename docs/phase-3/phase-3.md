# Phase 3: Conditional Local Control Surfaces

Status: Planned

## Goal

Turn the Sources, Apps, and Text Input placeholders into useful,
capability-gated local controls. The milestones carry forward the
[archived Phase 2 proposal](../archive/phase-2/phase-2.md) (old P2-M1 to
P2-M5, Dropped) and adapt it to the Phase 2 server boundary: the Control API
Server owns every probe, cache, session, and TV request, and the GUI and MCP
reach these features only through the same application policy.

The phase does not claim generic TV-state monitoring. The local WebSocket can
return a catalog, report an IME event, or accept a write. It cannot report the
active source, the foreground app, or what a text field now contains.

## Entry conditions

- Phase 2 is `Done`: one server-owned control path, the API and MCP
  surfaces, and the signed bundle are accepted.
- Each milestone has an overview and an architecture record before
  implementation, as the [development process](../development-process.md)
  requires.
- The research message shapes come from third-party clients, not Samsung
  documentation. Code that depends on a frame shape follows P3-M1's
  target-TV evidence, not the research alone.

## Product decision

Use the paired local remote connection as the only new TV transport. It keeps
the app on the local network and needs no account, backend, OAuth redirect,
public webhook, or cloud-token storage. SmartThings, Matter, local JSON-RPC
source control, and a companion Tizen app remain future independent
providers; see the
[control and monitoring capability map](../research/samsung-tizen-control-monitoring-capability-map.md).

## Phase-wide rules

- **Two separate vocabularies.** A *capability* says whether a feature may be
  offered for a TV. A *request outcome* says what happened to one request.
  Request outcomes reuse Phase 2's terminal set, `not_sent`, `written`, and
  `uncertain`, with "requested" as the user-facing wording for a write. A
  capability never implies an outcome, and a written request never implies an
  observed TV state.
- **Evidence has a source.** Every capability fact records how it is known:
  a TV response or event in this session, the user's own confirmation on this
  TV, or nothing. Where no automatic proof exists, as for source keys, the
  plan says so and does not invent one.
- **No automatic replay.** An `uncertain` source change, app launch, or text
  send is never retried automatically. A text resend can duplicate typed
  text. Wake and a follow-up launch or source change remain separate user
  requests.
- **Known inbound frames only.** Infrastructure parses only named Samsung
  events with strict, size-bounded schemas, such as the installed-app reply
  and IME start/end. It ignores unknown frames and never treats them as state.
  Wire shapes and event names stay in infrastructure.
- **Untrusted TV data.** App names and IDs come from the TV. Bound their
  length and count, and strip control and bidirectional-override characters
  before storing or rendering them. MCP results return them as data, never as
  instructions.
- **Per-TV, removable storage.** Source labels and app catalogs belong to one
  saved TV, are server-owned, and are deleted by Forget inside its existing
  crash-recovery transaction, so an interrupted Forget leaves no orphaned
  labels or catalog. A storage format change follows Phase 2's migration
  rule: explicit, tested against existing records, and recoverable from a
  documented backup.
- **Same contract for both clients.** Mutations carry the observed server
  epoch, selected-TV generation, and operation key, so stale selections are
  rejected and repeated keys cannot write twice. Setting changes, such as
  source labels and visible ports, also carry the config revision, as
  `set_wake` does. Capability gating appears in results and state, not by
  removing MCP tool definitions.
- **MCP budget.** The M10 catalog is already over its 1,900-token target by
  the recorded proxy measure, so there is no headroom. Prefer extending
  existing tools (for example `remote_action`'s finite key set and
  `inspect_tv`) over new ones. Any addition needs a measured offset or an
  owner-approved re-baseline before it ships.
- **Accessibility per milestone.** Each GUI change ships with keyboard
  access, VoiceOver labels, and disabled-state explanations. P3-M5 reviews
  them together.

## Milestones

| ID | Milestone | Deliverable | Status | Record |
| --- | --- | --- | --- | --- |
| P3-M1 | Capability model and protocol evidence | Target-TV frame evidence; server-owned per-TV capability model with evidence source, staleness, and invalidation; API and MCP read access. | Planned | — |
| P3-M2 | Sources | User-configured, labelled direct-input keys and the Sources menu fallback, reported as requests only. | Planned | — |
| P3-M3 | Apps | Per-TV installed-app catalog with bounded refresh and launch by a returned ID only. | Planned | — |
| P3-M4 | Text Input | Server-owned IME session that sends UTF-8 text to a focused native Tizen field without retaining it. | Planned | — |
| P3-M5 | Integrated resilience and accessibility | Cross-feature fakes, two-client and restart behavior, budget re-measurement, native review, and the target-TV matrix. | Planned | — |

Work in order: P3-M2 to P3-M4 depend on P3-M1's model and evidence, and P3-M5
verifies them together. P3-M2 to P3-M4 are otherwise independent; a recorded
P3-M1 result that a feature is unsupported on the target TV narrows or drops
that milestone rather than blocking the others.

## Milestone scope

These are starting contracts. Each milestone overview refines them and owns
its acceptance checklist.

### P3-M1: Capability model and protocol evidence

- Run a target-TV spike before fixing any schema. Record the actual
  installed-app reply, launch reply or silence, IME start/end events, and
  each direct source key's effect, then turn them into sanitized fixtures.
  Commit no addresses, tokens, typed text, or account data.
- Add a domain and application capability model, separate from connection
  state and remote-action eligibility. Each fact names its saved TV, evidence
  source, time, and any firmware clue the TV actually reports.
- Keep session facts, such as an active IME, only for the current connection
  and selection generation; clear them on disconnect, re-pair, selection
  change, Forget, and server restart. Persisted facts, such as an app
  catalog, show as stale after their expiry and refresh only on an explicit
  or policy-defined trigger.
- Expose capabilities through the typed HTTP API, events, and an existing MCP
  read surface, and add deterministic fakes for each recorded frame.

### P3-M2: Sources

- No probe can prove a source key. The TV accepts the write and reports
  nothing. Direct inputs are therefore user-configured per TV: the user
  chooses which HDMI ports to show and may label each one (for example
  "PlayStation"). The app never invents device or cable names.
- Offer the TV-tuner key and the Sources menu as ordinary keys. The menu is a
  manual D-pad fallback, never a scripted traversal.
- Send direct keys through the existing finite remote-action path so
  admission, queue limits, and outcomes match Phase 1. Report "Source request
  sent"; never "HDMI 1 active".
- Let the user hide a port that does not work on their TV. Record the
  hardware result for each offered key, including after Wake and reconnect.

### P3-M3: Apps

- Request the installed-app list only while connected. Refresh it on explicit
  request, after a launch failure, or on first connection once the cached
  catalog has expired, never before every launch. Coalesce concurrent
  refreshes from several clients into one request.
- Persist only the app ID, a sanitized display name, and the last-verified
  time per TV. Show staleness and the refresh time.
- Launch only an ID that TV returned, as an ordinary home launch with no
  deep-link parameters. Treat launch as a request ("Launching *name*…"). A
  clear failure marks the entry unavailable and offers refresh.
- Exclude guessed IDs, a shared app catalog, content deep links, credentials,
  and search-text launches.

### P3-M4: Text Input

- The server owns one text-input session per connection, opened by a TV IME
  start event and closed by IME end, `SendInputEnd`, cancel, disconnect, or
  selection change. A send names the session it observed and fails on a
  stale one. If P3-M1 shows the target TV sends no IME events, any explicit
  "send to focused field" fallback needs an owner decision and a visible
  warning.
- Encode the original UTF-8 as Base64 with a bounded length. If splitting is
  needed, split only on Unicode scalar boundaries. Never auto-submit a form
  (`KEY_ENTER` stays a separate action).
- Text and its Base64 payload never appear in logs, persistence, events,
  operation records, or MCP results. The GUI offers a masked entry mode and
  no history, and does not read the clipboard.
- MCP text goes through the model's context. The M4 overview records whether
  MCP gets text input at all and documents that it is unsuitable for
  passwords.
- Test ASCII, Korean, mixed scripts, symbols, and emoji. Document that
  streaming-app letter grids still need D-pad input.

### P3-M5: Integrated resilience and accessibility

- Verify selection changes, reconnects, re-pair, Forget, Wake, stale catalogs,
  cancellation, queue pressure, uncertain writes, server restart, and a
  crash during Forget across
  all Phase 3 requests from the GUI and MCP at once.
- Extend fakes for malformed, oversized, and late frames, send failures,
  timeouts, and reconnects, and test saved-data migration against existing
  records.
- Re-measure the MCP catalog and workflow budgets with Phase 3 included.
- Run the [contribution-guide](../contribution-guide.md) checks and the
  target-TV matrix. Complete a native macOS review of keyboard access, focus,
  VoiceOver labels, disabled explanations, and the minimum window size.

## Open decisions

- Whether MCP receives text input (P3-M4) and whether source-label editing is
  GUI-only or also exposed through MCP (P3-M2).
- The app-catalog expiry and size bounds, and the text length limit, fixed in
  each milestone overview from P3-M1 evidence.
- Whether a sensitive-entry mode is enough for passwords, or password fields
  should only be typed on the TV.
- How operation-key deduplication handles text sends. The operation registry
  keeps a payload fingerprint, and a hash of short text such as a PIN can be
  reversed by guessing. Text must be excluded from it, or the fingerprint must
  be keyed and short-lived.
- Whether Phase 3 MCP additions are paid for by catalog cuts or by an
  owner-approved budget re-baseline.

## Non-goals

- SmartThings account linking, OAuth tokens, cloud backend, or webhooks.
- Matter commissioning, local JSON-RPC source control, or an owned companion
  Tizen TV app.
- Generic active-app, title, episode, source, panel-power, or playback-state
  monitoring.
- Streaming-app grid typing, blind remote macros, or app-specific deep links.
- Pointer/mouse frames, Art Mode, voice input, and other unspecified
  inbound events.
- Commercial-display RS-232/MDC control.

## Phase exit criteria

- Sources, Apps, and Text Input replace their placeholders. Each control is
  available only when its capability rule holds for the selected TV, and
  otherwise explains why it is not.
- A write is never shown or returned as an observed TV state, and no
  `uncertain` Phase 3 request is replayed automatically.
- All new local data is per-TV, stale-aware, removed by Forget,
  server-owned, and excludes secrets and user-entered text.
- GUI and MCP reach every new feature through the Control API Server with the
  same validation, outcomes, and budgets.
- Automated tests use fakes built from recorded, sanitized frames. The
  target-TV matrix records feature-by-feature evidence without sensitive data.
- The contribution-guide checks pass before the phase closes.

## Research basis

- [Capability map](../research/samsung-tizen-control-monitoring-capability-map.md)
- [Source changes and app launches](../research/source-changes-and-app-launches.md)
- [Text input from a macOS remote](../research/samsung-tv-text-input.md)
- [Tizen TV state monitoring](../research/tizen-tv-state-monitoring.md)
