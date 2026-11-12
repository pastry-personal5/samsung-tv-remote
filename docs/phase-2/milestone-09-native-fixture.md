# P2-M9 synthetic native fixture

Status: Fixture self-checks pass; signed native acceptance has not run.
This serves the real Samsung adapter's HTTPS metadata and secure WebSocket
routes without a physical TV. It never contacts a TV, sends Wake packets,
opens application profiles or accesses Keychain. The fixture's pairing token
is an explicitly public synthetic constant. No live credentials belong here.

## Prepare an isolated endpoint

For native Local Network attribution/denial/recovery, use a separate controlled
host on the same private LAN. A listener on the client's own Mac or loopback
does not establish this OS gate. No suitable external fixture host has been
identified or contacted. The script has no discovery/broadcast behavior.

On a Mac fixture host, generate a short-lived fixture certificate in a private
temporary directory. Do not install it in a trust store or reuse any existing key:

```sh
umask 077
m9_fixture_dir=$(mktemp -d /private/tmp/samsung-m9-tv.XXXXXX)
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
  -subj '/CN=M9 Synthetic TV' \
  -keyout "$m9_fixture_dir/key.pem" -out "$m9_fixture_dir/cert.pem"
/usr/bin/openssl x509 -in "$m9_fixture_dir/cert.pem" -noout -fingerprint -sha256
```

Record the certificate fingerprint privately to compare with the GUI's Check
TV observation. Set `m9_fixture_address` locally to that host's explicit private
IPv4 interface; the script rejects wildcard, multicast and public bindings.
It uses port 8002, as the production Samsung adapter does. An occupied port
fails; the fixture does not kill an owner or choose another port.

```sh
python3 scripts/m9_tv_fixture.py --bind "$m9_fixture_address" \
  --cert "$m9_fixture_dir/cert.pem" --key "$m9_fixture_dir/key.pem"
```

Keep its foreground console open. It emits fixed event names and accepts:

| Console input | Behavior |
| --- | --- |
| `approve` | New Pairing receives a synthetic token; reconnect with that token receives remote readiness. |
| `deny` | New channel attempts receive `ms.channel.unauthorized`; existing authorized channels stay open. |
| `offline` | Existing sessions disconnect and new connections close. |
| `quit` or EOF | Stop the listener and sessions. |

The fixture bounds concurrent connections, headers and frames. It accepts
masked client frames, responds to Ping/Close and counts a semantic remote
request only as `synthetic_key_received`. It never performs a real action.
Logs contain no peer addresses, request URLs, tokens, key names or payloads.
Idle sessions close after 120 seconds; keep individual review steps shorter
than this or record the fixture timeout as such. It does not emulate SSDP,
real TV consent, firmware behavior or physical standby/panel power.

## Native review after signing/access approval

Use the selected final signed bundle and its actual production server. Do not
run this against a real TV's saved row, clear credentials, switch namespaces
or import legacy data. Preserve the preflight manifest and legacy hashes.

1. In Settings, manually Check the fixture host. Require the expected synthetic
   name/model and certificate fingerprint before confirming Pair. Record
   first Pairing, durable save and paired readiness separately. The fixture's
   approval stands in for TV consent, not for macOS consent or human identity.
2. Confirm one harmless remote intent by matching its terminal write result
   with one fixture `synthetic_key_received` event. This proves only delivery
   to the fixture. Close/reopen the GUI, then restart/rebuild using explicit
   lifecycle controls and verify saved-profile/credential restore with zero
   routine Keychain prompts. Do not repeat the remote intent to test recovery.
3. To review Wake readiness, choose an intentionally synthetic, locally
   administered MAC that belongs to no real device. Configure it explicitly.
   Enter `offline` in the fixture, wait for the GUI to observe disconnection,
   request Wake once and inspect the reported packet phase. Enter `approve`
   before the readiness deadline so the existing operation can reconnect.
   The fixture does not listen for or count magic packets. Cite deterministic
   send-count tests for that invariant; do not infer packet delivery or panel
   power from this fixture. Record cancellation before/during/after send with
   the server's precise packet facts when exercising those cases.
4. Review Local Network attribution and denial/recovery through the Mac's
   native permission UI with the owner present. Record which signed app is
   attributed, detached server behavior, visible failure and recovery after
   explicit retry. Do not reset permissions for other apps. Fixture `deny`
   simulates TV authorization failure and does not simulate OS network denial.
5. Review native blocked/locked/interaction-required credential states using
   an agreed isolated procedure. Never lock the owner's whole login keychain
   or alter unrelated access controls merely to manufacture evidence. The
   standalone handoff helper is an explicit approval workflow; it is not a
   substitute for routine UI-off access or a credential-failure test.

Record bundle/revision, epochs, prompt counts, private fixture identity,
expected/actual results and sanitized screenshots. Keep IP/MAC addresses,
fingerprints, private paths and tokens out of committed evidence. Do not mark
the gate passed from this runbook or the fixture's own tests. After review,
use the GUI's explicit Forget/recovery workflow for the synthetic row if
cleanup is intended, then stop the fixture. Do not manually delete profile or
Keychain items as cleanup.

## Automated fixture checks

```sh
python3 -m unittest discover -s tests -p 'test_m9_tv_fixture.py' -v
```

These tests use generated one-day fixture certificates, ephemeral loopback
listeners and in-memory frames. They check metadata, Pairing/reconnect,
denial, Ping/Close, key receipt, offline/recovery, frame limits and log
redaction. They require no TV, LAN host, app launch, Keychain or permission
changes and do not establish signed production acceptance.

The delegated run on 2026-11-12 passed all 8 tests; complete output is retained
privately in `/private/tmp/samsung-m9-tv-fixture-tests.log`. Syntax compilation,
CLI help and rejection of wildcard/public/multicast binds also passed. Parser
limit tests use in-memory streams to avoid blocking a small OS socket buffer
before a reader starts; TLS interaction tests use actual loopback sockets.
