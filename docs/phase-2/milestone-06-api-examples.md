# Control API examples

M8 `/v1` examples for the isolated fake profile. Production GUI cutover is M9;
MCP tools are M10. All addresses, IDs and fingerprints here are synthetic.
The [wire decisions](milestone-08-wire.md) define exact encodings and bounds.

In the table, `E` means the current full canonical epoch, `K` a new canonical
UUID generated once for an explicit intent, `A` a saved device UUID, `O` an
operation UUID and `T` the exact shared recovery transaction UUID. Each mutation
body includes `server_epoch:E, operation_key:K`. All generations, revisions and
UTC millisecond expiries are **decimal JSON strings**, including `"0"`.

| Operation | Route and additional body fields | Receipt / outcome |
| --- | --- | --- |
| First use | `GET /v1/state` | Empty devices, null selected ID, generation `"0"`; no legacy reads, discovery or credential access. Setup requires explicit intent. |
| Inspect | `GET /v1/devices/A` | Saved host/label, `config_revision`, Wake settings; no token or physical power claim. |
| Discover | `POST /v1/discoveries` | Pending receipt, then private `result.kind:discovered` with at most 32 expiring candidate IDs and hosts. |
| Probe | `POST /v1/probes {host:"synthetic-tv.invalid",candidate_id?:C}` | `probed`: equal attempt/observation IDs, full fingerprint, host, bounded name/model and stable display expiry. |
| Pair | `POST /v1/pairings {attempt_id:O,observation_id:O,host:"synthetic-tv.invalid",fingerprint:F,confirm_fingerprint:F,label:"Synthetic TV",selection_generation:"0"}` | TV approval and durable storage yield `tv_approved_and_saved`; acceptance alone promises neither. |
| Re-pair | Same route plus `device_id:A`, current generation | Same saved ID only after fresh observation, exact fingerprint and TV approval. Interrupted re-pair also requires `recovery_transaction_id:T`. |
| Select | `POST /v1/selection {device_id:A,expected_generation:"7"}` | `saved`; durable selection increments generation. A stale new intent has a retained terminal rejection. |
| Connect | `POST /v1/connections {device_id:A,selection_generation:"7"}` | `remote_ready` means observed session readiness; no power claim. |
| Launch Connect | `POST /v1/connections {device_id:A,selection_generation:"7",only_if_idle:true}` | Owner checks disconnected/idle state atomically; a ready or competing operation yields retained `busy`. The GUI does not retry the launch intent. |
| Wake configuration | `PUT /v1/devices/A/wake {device_id:A,selection_generation:"7",expected_config_revision:"3",wired_mac:"02:00:00:00:00:01",wifi_mac:null,active_interface:"Wired"}` | `saved` with stable terminal selection/configuration facts; no packet. |
| Remote action | `POST /v1/remote-actions {device_id:A,selection_generation:"7",action:"VolumeUp"}` | `written`, `not_sent` or `uncertain`. PlayPause resolves at dispatch. PowerToggle probes before Wake if disconnected. |
| Wake | `POST /v1/wakes {device_id:A,selection_generation:"7"}` | `wake` contains separate packet and remote-readiness facts. Finding a reachable TV uses `connected`/`packet:not_sent`; `ready` follows a sent packet. |
| Cancel Wake | `POST /v1/wakes/O/cancel {device_id:A,selection_generation:"7"}` | Cancels this named operation only; preserves any already-sent or uncertain packet fact. |
| Forget | `POST /v1/devices/A/forget {device_id:A,expected_config_revision:"3",confirm_device_id:A}` | `forgotten` only when all removals complete; otherwise `partial_setup` plus per-store facts and recovery marker. |
| Recover first Pair | `POST /v1/devices/A/recover-pairing {device_id:A,transaction_id:T,confirm_device_id:A}` | Compensates only the exact reserved ID/transaction. Re-pair uses fresh approved Pairing instead. |
| Recover Forget | `POST /v1/devices/A/recover-forget {device_id:A,transaction_id:T,confirm_device_id:A}` | Completes only this marker's remaining removals, including when the saved record is gone. |
| Lookup | `GET /v1/operations/O?epoch=E` or `GET /v1/operations/by-key/K?epoch=E` | Same identity only. Old epoch: `unknown_after_restart`; unretained current key: `outcome_unavailable`. Neither establishes that no effect happened. |
| Events | `GET /v1/events` | Atomic snapshot, then every consecutive shared revision. Private operation input/results are absent. Overflow requires fresh authentication/snapshot, with no mutation replay. |
| Stop | `POST /v1/server/stop` with only common envelope, GUI management identity | `durable_stop:true` follows synced marker persistence; admission is fenced first. Lost reply never cancels Stop. |
| Resume / restart | Explicit GUI launcher action; no stopped HTTP listener | Resume waits for ownership/settlement and clears Stop. Incompatible live builds require explicit restart. MCP cannot manage either action. |

Canonical remote request:

```json
{"server_epoch":"e1111111-1111-4111-8111-111111111111","operation_key":"a2222222-2222-4222-8222-222222222222","device_id":"d3333333-3333-4333-8333-333333333333","selection_generation":"7","action":"VolumeUp"}
```

A pending receipt has no expiry or terminal result:

```json
{"operation_id":"53545201-0100-0000-0000-000000000001","server_epoch":"e1111111-1111-4111-8111-111111111111","device_id":"d3333333-3333-4333-8333-333333333333","selection_generation":"7","state":"pending","result":null}
```

A terminal result retains its metadata and stable expiry. For example,
`result:{"kind":"uncertain"}` is a socket uncertainty fact. The client keeps K
for GET lookup; it never creates a new key or resubmits the mutation after a
lost HTTP reply, reconnect, epoch change or expired retention. An explicit new
user intent is required before repeating a possible effect.

`tests/control_wire.rs` checks every mutation shape against the real parser and
native encoder. `tests/control_response.rs` covers all typed outcome/error
projections; `tests/control_http.rs` exercises actual authenticated routing,
private lookup, shared events, duplicate/lost replies and guarded recovery.

Canonical GUI Stop body (management identity is authenticated, never supplied
in JSON):

```json
{"server_epoch":"e1111111-1111-4111-8111-111111111111","operation_key":"a2222222-2222-4222-8222-222222222222"}
```
