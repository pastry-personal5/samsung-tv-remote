# M8 bootstrap transcript and boundary review

Date: 2026-11-12. Implemented fake-profile protocol; signed code identity and
production API-credential provisioning remain M9. This is an implementation
review, not independent cryptographic or signed-platform certification.

The API credential port returns separate random 256-bit GUI and MCP secrets.
The development process stores these only in memory. GUI is stable identity 1
with management capability; MCP is stable identity 2 with control capability.
Neither reusable secret appears in argv, environment, stdout, metadata, events
or logs. Pairing uses an entirely separate synthetic credential store.

## Transcript

1. Resolve the canonical fake-profile namespace through checked directory
   handles. Its full path SHA-256 binds the profile; a shortened hash names the
   owner-only Unix socket directory. The profile owner holds its lifetime lock
   before binding or removing a verified stale socket. Client and server check
   the Unix peer through the injected `PeerVerifier` port.
2. Client sends a framed strict `Hello`: version, role, random 256-bit nonce and
   profile binding. Server verifies peer, profile/version and nonce freshness,
   then loads the role's API secret without prompting. A revoked identity is
   rejected; reconnect does not regrant its stored secret.
3. Protected Unix channel returns a fresh challenge UUID, echoed nonce/role,
   endpoint/build/API/epoch/profile context, a one-use readiness bearer and a
   different random proof key. The reusable API secret is withheld.
4. Client sends **only** the one-use bearer and challenge UUID to the literal
   loopback `/v1/ready`. It cannot authorize state, mutation or event routes.
   The actual listener consumes the grant once and returns HMAC-SHA256 of the
   canonical JSON tuple `(domain, version, role, nonce, challenge, profile,
   endpoint, build, API, epoch)`, using the protected-channel proof key. Domain
   is `samsung-tv-remote/bootstrap-ready/v1`.
5. Client checks the proof in constant time and matches all readiness context.
   A build mismatch blocks before redeem. It sends a matching `Redeem` over
   the original protected channel. Server requires consumption of this grant,
   unexpired challenge, exact proof and a still-current role credential.
   A proved nonserving listener or busy readiness is transient and releases no
   API credential. A rejected one-use grant starts a new protected challenge;
   it never renews the old nonce/grant. Transport loss is also transient.
6. Only then does the protected channel return the reusable role bearer.
   HTTP/SSE connections continue using revocable non-secret admission leases;
   rotation preserves identity, invalidates old credentials and closes streams.

Frames are length-prefixed JSON, at most 4,096 bytes, with strict unknown-field
and duplicate-field rejection. Reads/writes have five-second deadlines; redeem
has ten seconds. Client's complete exchange has a five-second deadline. There
are 16 protected channels, 32 readiness grants and 128 retained nonces; grants
and nonces expire after 30 seconds. Saturation refuses work; it does not evict a
live nonce or extend freshness. Secrets are generated with `getrandom`, and
`hmac` implements the standard HMAC construction; no custom hash primitive is
used. Reference: [RFC 2104](https://www.rfc-editor.org/rfc/rfc2104).

## Review findings and evidence

An unrelated occupied TCP listener receives a disposable readiness credential,
never the reusable API bearer or proof key. It cannot calculate the bound
proof. `occupied_http_listener_never_receives_a_reusable_credential` runs this
attack with real TCP and Unix sockets and asserts that both reusable fixture
secrets are absent from the captured HTTP request.

`tests/server_bootstrap.rs` also covers wrong UID, injected code-identity
rejection, interaction-required credentials, replayed nonce, unconsumed grants,
revoked identities, wrong proofs, every bound context field, one-use readiness,
frame size, expiry and nonce capacity. Rotation between challenge and redeem
expires that exchange while a fresh exchange obtains the current secret;
revocation remains denied. `control_client_rotation` checks the actual GUI
actor's refresh/fresh snapshot and denial block without replaying a TV write.
The socket-publication unit fixture verifies mode before visibility and an
exclusive rename preserving a competing live socket. `tests/server_access.rs`, the owner
admission unit test and actual HTTP/SSE fixtures cover rotation, revocation,
capabilities and the admission ordering boundary.

The **development** peer adapter checks only effective UID. A malicious process
running as that user can impersonate a development protected peer; owner-only
paths and HMAC do not fix this. M8 never installs this adapter in production.
M9 must supply and test signed peer/code identity and fresh production
Keychain credentials, including locked/denied/interaction-required behavior.
Signed upgrade and user-session integration remain M11. No M8 fake result is
claimed as evidence for those gates.
