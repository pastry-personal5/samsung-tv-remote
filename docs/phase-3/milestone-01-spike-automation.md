# P3-M1 physical-TV spike automation

Status: Active — offline recorder and feature-gated owner/controller implemented; hardware evidence pending.

The review request authorizes preparing the diagnostic workflow. It does not
accept Phase 2, freeze a Samsung schema, or start a hardware experiment. No
physical-TV result is recorded by this document.

## Available diagnostic candidate

The [private diagnostic contract](milestone-01-diagnostic-contract.md) freezes
bounds and ownership for the implemented `p3-spike` Cargo feature. Default
builds have no diagnostic routes/controller. Build a preserved Apple-signed
candidate with `make bundle-p3-spike-candidate`; this never installs it or
switches a running owner. The bundle records commit/dirty provenance before
signing and is created under a new `.noindex` candidate directory.

The foreground controller command is:

```sh
'/absolute/path/to/Samsung TV Remote.app/Contents/MacOS/samsung-tv-remote' \
  --p3-spike baseline /absolute/absent/evidence-draft-directory
```

Use `reconnect` or `wake-reconnect` for the other conditions. The output
directory must not exist. Attachment requires the same signed candidate's
already-running owner and a selected, paired, ready TV. The controller cannot
start/resume an owner, pair, connect or clear Stop. Switching owners is a
separate reviewed action affecting all clients; reconnect and manual Wake
remain explicit operator steps.

Inside the controller, `catalog` admits one finite acquisition. It displays
bounded quoted names and numeric choices, never app IDs. `aliases 0 1` selects
two distinct returned choices for A/B; choose harmless apps after reviewing
the actual names. `ime` opens a passive 30-second observation window: manually
open/close a harmless native field and do not type. Every `confirm hdmi1`,
`confirm hdmi2`, `confirm hdmi3`, `confirm tuner`, `confirm sources`,
`confirm launch-a` or `confirm launch-b` is one separately intentional effect
for this lease/session. No batch or previous-session intent is replayed.

`status` and `export` send no TV request. `quit`, EOF or controller loss closes
the lease and aborts its session. Explicitly reconnect before reacquiring a
lease or a quarantined family. An ambiguous catalog is usable for diagnostic
alias selection, but never establishes product capability support. Launch
silence quarantines launching, so B may require a fresh explicit reconnect,
catalog and alias choice; document that extra fence rather than replaying A.
The owner retains bounded outcomes; a missing reply always remains unknown.

Exports are private JSON drafts containing machine records, sanitized fixture
subsets, digests, exact executable hash, commit/dirty/signature provenance and
UTC epoch times. Real app names/IDs, field payloads, actual lease/intent IDs and
network details are removed. Fixtures retain reviewed event names and data
types; unknown fields are omitted. Every draft is marked review-required and
diagnostic-only. Fake-profile exports are labelled synthetic, never captured.
Human-visible results belong in the offline matrix and evidence review.

## Available now: matrix preparation and operator recording

`scripts/p3_spike_record.py` automates the 30-row matrix, validated incremental
records and Markdown reporting. It opens no socket, reads no profile or
Keychain, and sends no request. Its strict schema accepts a full Git build ID,
a dirty flag, UTC date and finite observations only. It rejects extra fields,
duplicate JSON keys, oversized files and free-form payloads. App A/B are local
aliases; no address, token, app ID or field content belongs in this recorder.

Run from the repository root, substituting the **tested executable's** full
build commit ID (not necessarily the checkout's HEAD):

```sh
python3 -B scripts/p3_spike_record.py prepare /private/tmp/p3-spike-00.json \
  --build FULL_BUILD_COMMIT_ID --date 2026-11-14
python3 -B scripts/p3_spike_record.py record /private/tmp/p3-spike-00.json \
  /private/tmp/p3-spike-01.json --row baseline/hdmi1 --result observed
python3 -B scripts/p3_spike_record.py render /private/tmp/p3-spike-01.json
```

The build placeholder must be replaced with 40 or 64 lowercase hexadecimal
digits; use `--dirty` if the tested build contains local changes. Every output
path must be absent. Each record command preserves its input and changes only
one row; continue from the newest output. `render` validates before printing.
Report output is suitable for review, but is not a protocol fixture or a
complete evidence record. A prepared matrix contains only `untested` results.

| Value | Operator declaration |
| --- | --- |
| `untested` | No experiment performed; the initial value. |
| `observed` | Operator observed the expected event or visible effect for this row. Must be corroborated separately for protocol acceptance. |
| `not_observed` | Expected event/effect was not observed in the documented window; support remains inconclusive. |
| `inconclusive` | Experiment attempted but outcome cannot be determined. |
| `not_applicable` | Operator declares the prerequisite absent (for example no tuner setup); the scope decision remains open. |

The recorder deliberately cannot declare `supported` or `unsupported`, accept
frames or claim machine measurements. Record observation windows, actual
manual steps, signing/build verification and fixture provenance separately in
`milestone-01-evidence.md` when hardware work begins. Do not commit a generated
empty matrix as purported hardware evidence.

## Implemented live automation contract

The ordinary `TvSessionControl` API supports clicks and abort. Feature builds
add a once-taken diagnostic port for the private lease. The normal event stream
has semantic IME lifecycle values for foundation fakes, but the production
Samsung adapter emits neither; catalog/launch observations stay in the
diagnostic port. `RemoteAction` has no HDMI/tuner/Sources
variants. The running owner exposes no diagnostic lane. Existing Python port
and pairing probes open independent connections and are unsuitable here.
The ordinary public API still cannot perform these experiments. The private
feature-gated controller uses the existing owner rather than a second socket.

The following **server-owned diagnostic slice precedes the spike**,
under the predecessor gate or an explicitly scoped diagnostic exception. It
may use research-shaped candidate encoders/parsers to test hypotheses; mark
them diagnostic-only. Production parsers still require reviewed TV evidence.
Do not expose generic JSON injection or add tools to MCP.

1. An explicitly enabled signed diagnostic build attaches a protected local
   foreground controller to the existing owner. Authenticate it using existing
   local bootstrap/peer checks; disable the lane in ordinary builds/startup.
   The [private contract](milestone-01-diagnostic-contract.md) and fake tests
   define that interface.
   Opening this controller never starts/resumes an owner or pairs a device.
2. Acquire a single diagnostic lease for one selected saved TV. Check epoch,
   selection generation, session generation, trusted pin and pairing lineage
   at admission and completion. The runtime owns the lease and sole socket.
   Concurrent diagnostics fail busy. Ordinary TV mutations fail busy during
   the lease; readers remain available. Capture target identity at admission.
3. Admit only finite catalog, launch-A/B, IME-observe and individual source-key
   experiments. Launch accepts only an in-memory alias from the current
   session's validated catalog, never an external or guessed app ID. No text
   send, Enter, Power toggle, arbitrary key or deep link is admitted.
4. Before each source or launch request, require foreground operator intent
   for that named experiment and current target/session. Batch approval does
   not authorize later effects after a fence. Abort or controller loss clears
   intent and the lease. It cancels unsent work; an admitted write retains its
   outcome and is never replayed. Reserve capacity for abort/disconnect.
5. Automate bounded observation windows, measurement, sanitized extraction and
   reporting after intent. Use monotonic deadlines, bounded queues and a total
   run limit. Initial diagnostic ceilings: one outstanding experiment, 10-second
   catalog/launch windows, 30-second IME windows, 4-second write deadline,
   16 KiB inbound message/frame cap and 30-minute run limit. Freeze command,
   result and queue capacities before code. If a limit is hit, record it as
   inconclusive and stop that lane; do not silently raise production limits.
6. A wire reply without a request ID can only be associated with the sole
   matching outstanding request in this session; label that association rather
   than inventing a TV-echoed ID. On timeout, quarantine that request family
   until an explicit reconnect so a late reply cannot satisfy the next request.
   Single-flight alone cannot exclude unsolicited/duplicate events: label
   indistinguishable responses ambiguous and do not promote them to automatic
   capability evidence. Never infer launch success from an unrelated event or
   catalog response.
7. Clear aliases and pending correlation on every fence. An unexpected
   disconnect aborts the case. Reconnect is a new explicit step, never a retry
   of an uncertain operation. Rediscover apps before selecting A/B again.
8. Disable raw-frame and payload Debug logging. Extract only reviewed event
   names, envelope/type information, byte/count metrics, bounded synthetic
   app replacements and IME lifecycle markers. Unrecognized events produce
   counts only; do not retain their keys or strings. Sanitization must happen
   before persistence/output, not as an optional post-processing command.
   Test sensitive marker strings at every output/error boundary. An unreviewed
   shape stays in memory or is discarded; any new schema needs review before
   an allowlisted structural fixture can be exported.
9. Emit an evidence draft and fixture manifest with synthetic target/session
   aliases, UTC observation date, exact build/dirty/signing verification,
   experiment, window, request outcome, association method, observation source,
   measured sizes, redaction rules and fixture digests. Never include pins,
   network details, tokens, app IDs, field identifiers or typed/Base64 content.
   Operator observations are separate fields from machine observations.
   Review before committing; never import a report into runtime capabilities.

Prove the controller/owner contract with fake sessions first: duplicate intents,
controller death, queue pressure, late replies after timeout, same-TV
reselection, reconnect, cancellation during write and two competing clients.
Require exact effect counts and zero network effects from report/read paths.
A diagnostic exception covers this slice only; it cannot accept production
capability projections or waive native/budget acceptance.

## Hardware sequence and remaining human work

Run three conditions: baseline, explicit reconnect, then manual Wake followed
by explicit reconnect. Keep Wake a separate operator action; do not power-cycle
a TV automatically or replay a previous request after Wake.

For each condition, acquire fresh catalog evidence and choose two returned IDs
as A/B. The harness sends each approved ordinary launch once and records reply,
error or deadline independently of the operator's visible result. If fewer
than two safe apps exist, mark the missing row untested and record the scope
constraint. Never open an app requiring account interaction just to fill a row.

The operator manually opens and closes a harmless native field while the
harness watches for IME lifecycle events. No typing is needed in M1. Record end
without start, duplicates and activity reset at reconnect. A missing event is
inconclusive; do not invent an empty or unsupported fixture.

For HDMI 1–3, tuner and Sources menu, the harness sends only the individually
approved finite key. The operator verifies the panel and records the result;
there is no protocol proof of active input. Test only configured/connected
inputs, note absent prerequisites, and navigate the chooser manually. Screen
vision, blind macros, clipboard reads and automatic form submission are outside
this workflow. Return to a safe state manually and release the diagnostic lease.

## Evidence review and handoff

The offline recorder covers operator declarations only. The live harness must
supply wire observations and measured limits. A reviewer reconciles both into
the evidence record, checks redaction and labels every fixture as captured and
sanitized or synthetic. Preserve types and encoding behavior in replacements;
never substitute a synthetic missing IME event for a hardware capture.

Only then freeze production event shapes, correlation policy, negative-evidence
rules and size limits. If the TV returns no usable evidence, record a scoped
outcome and keep capability support unknown. M2–M4 may be narrowed by an
explicit decision. Passing offline tests cannot close this hardware gate.
