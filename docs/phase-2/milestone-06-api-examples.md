# P2-M6 `/v1` operation examples

These are synthetic **contract examples**, not responses from a running
server. Each mutation also carries the authenticated client's identity from
the access gate, never from JSON. `E`, `A`, `B`, `K`, `P`, `O` below abbreviate
UUID values only to keep the table readable; real DTOs use full UUID strings.
`G=7` is the client's observed generation and `R=3` a saved configuration
revision. Examples are independent, not one chronological transcript. The
[contract](milestone-06-contract.md) defines the route and error map.

| Operation | Example request fields after authentication | Receipt / terminal fact |
| --- | --- | --- |
| Authenticated readiness | `GET /v1/ready` | `{server_epoch:E,api_version:"v1",build:"...",serving:true}`. This is not TV or Keychain readiness. |
| State query | `GET /v1/state` | `{server_epoch:E,revision:12,selected:A,generation:7,connection:"ready",wake:"idle"}` plus safe saved summaries. No network effect. |
| Inspect saved TV | `GET /v1/devices/A` | `{device_id:A,label:"Living room",config_revision:3,wake_active:"wired"}`; no pairing token, raw certificate or physical state claim. |
| Discover | `POST /v1/discoveries {server_epoch:E,operation_key:K}` | Receipt `O`; terminal `{candidate_id:"candidate-1",expires_in_seconds:120}`. Candidate is not paired. |
| Probe | `POST /v1/probes {server_epoch:E,operation_key:K,candidate_id:"candidate-1"}` or a validated local `host:"tv.local"` | Receipt `O`; terminal `{attempt_id:P,observation_id:"obs-1",fingerprint:"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",expires_in_seconds:300}`. |
| First Pairing | `POST /v1/pairings {server_epoch:E,operation_key:K,attempt_id:P,observation_id:"obs-1",confirmed_fingerprint:"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",label:"Living room",selection_generation:7}` | Receipt `O` says awaiting TV approval. Terminal is `tv_approved_and_saved` with new ID/generation, `tv_denied`, `stale_selection`, or `blocked_partial_setup`. The full fingerprint must exactly match the observation. |
| Re-pair | Same Pairing fields plus `replace_device_id:A`, `selection_generation:7` | Same ID A only after TV approval and durable commit; certificate/host mismatch or stale attempt cannot silently overwrite A. |
| Select | `POST /v1/selection {server_epoch:E,operation_key:K,device_id:B,expected_generation:7}` | Durable success publishes selected B/generation 8. Failure returns `stale_selection` with current selection, no write. Same-ID A also advances generation and restarts session. |
| Connect | `POST /v1/connections {server_epoch:E,operation_key:K,device_id:A,selection_generation:7}` | Receipt `O`; terminal `remote_ready`, `offline`, `pairing_required`, `certificate_changed`, or blocked. `remote_ready` is observed session readiness, not TV power state. |
| Wake configuration | `PUT /v1/devices/A/wake {server_epoch:E,operation_key:K,expected_config_revision:3,wired:"02:11:22:33:44:55",wifi:null,active:"wired"}` | Durable config revision 4. Stale revision or invalid MAC is rejected without packet. |
| Remote key | `POST /v1/remote-actions {server_epoch:E,operation_key:K,device_id:A,selection_generation:7,action:"VolumeUp"}` | Receipt `queued`; terminal `not_sent`, `written`, or `uncertain`. `written` is a socket fact only. |
| Play/Pause | Same route with `action:"PlayPause"` | Server resolves `Play` or `Pause` at dispatch from its inference; uncertainty resets inference, never claims observed playback. |
| Connected Power Toggle | Same route with `action:"PowerToggle"` | One queued power action; terminal is a remote-write fact, not observed power. |
| Disconnected Power Toggle | Same route with `action:"PowerToggle"` and valid Wake setup | One operation probes first; only offline/timeout leads to one Wake packet. Result reports packet status and separate remote readiness; other probe errors never Wake. |
| Wake | `POST /v1/wakes {server_epoch:E,operation_key:K,device_id:A,selection_generation:7}` | `{packet:"sent",remote:"not_ready"}` is possible; packet send does not prove power on. |
| Cancel Wake | `POST /v1/wakes/O/cancel {server_epoch:E,operation_key:K}` | `{packet:"sent",remote:"readiness_unknown",cancelled_reconnect:true}` preserves an already sent packet. |
| Forget | `POST /v1/devices/A/forget {server_epoch:E,operation_key:K,expected_config_revision:3,confirm_device_id:A}` | Complete only if token, trust and record removal all succeed. Partial example: `{outcome:"blocked_partial_setup",credential_removed:false,trust_removed:true,record_removed:true}`. |
| Operation by ID/key | `GET /v1/operations/O` or `GET /v1/operations/by-key/K?epoch=E` | Same-client receipt/result without replay. A changed epoch yields `unknown_after_restart`; an unretained same-epoch key yields `outcome_unavailable` because expiry and never-admitted cannot be distinguished. Neither means not sent. |
| Events | `GET /v1/events` | First event is an atomic snapshot at revision 12; later event revision 13 updates one fact. On lag, `resync_required` closes stream and client discards its projection. |
| Stop server | `POST /v1/server/stop {server_epoch:E,operation_key:K}` by the authenticated GUI management identity | Sticky marker is durable before success; pending work becomes `not_sent` and possible writes become `uncertain`. The listener closes after drain. A lost response is checked from the owner-only marker and readiness. |
| Resume server | Explicit signed-GUI launcher action while stopped; there is no HTTP listener then | Under the lifecycle lock, clears the sticky marker and starts/attaches. It never repeats a TV action. The MCP helper cannot resume. |

Example remote response sequence with valid synthetic UUIDs:

```json
{"operation_id":"f4444444-4444-4444-8444-444444444444","server_epoch":"e1111111-1111-4111-8111-111111111111","device_id":"d3333333-3333-4333-8333-333333333333","selection_generation":7,"state":"queued"}
```

```json
{"operation_id":"f4444444-4444-4444-8444-444444444444","device_id":"d3333333-3333-4333-8333-333333333333","outcome":"uncertain","meaning":"The TV may have received this action; do not retry automatically."}
```

An HTTP timeout after either body leads to operation lookup by K under the
same authenticated client identity. The GUI generates K from one user intent
and retains it through timeout/reconnect. MCP tools likewise require the
caller to supply K; server adapters cannot create a fresh K on retry.
