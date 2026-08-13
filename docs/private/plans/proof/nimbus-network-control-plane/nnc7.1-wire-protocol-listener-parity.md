# NNC7.1 Wire-Protocol Listener Parity

Status: `in progress; source audit complete; acceptance verification next`

Starting checkpoint: `fb9880e16b9b0a95768fe4f217318fa4532229aa`

NNC7.1 verifies that the existing protocol listeners consume the portable port
lease authority without moving their socket, protocol, or security effects.
NNC3.5 already migrated the main HTTP/WebSocket listener and sibling MongoDB,
DynamoDB, and S3 listeners. NNC3.6 already migrated standalone RESP. NNC4.6d
already injected one manager-derived authority into production composition.
This item closes the integration gate. It adds no second listener authority and
does not repeat those migrations.

## Current Ownership

```text
nimbus-network
  portable listener/lease IDs, claims, fences, durable provider evidence
       |
       +-- nimbus-server listener_lease adapter
       |     CLI owns main bind -> server owns HTTP/WebSocket serve
       |     server owns sibling bind -> WireProtocolAdapter owns guard/spawn
       |     nimbus-system receives observed listener rows after activation
       |
       +-- nimbus-kv listener adapter
             KV owns RESP bind/adopt/accept and loopback/auth policy
```

`WireProtocolAdapter` is a private server seam. MongoDB, DynamoDB, and S3 are
its three production implementations. It owns adapter identity, bind address,
the security guard, and adapter task construction. It does not open durable
state or call `nimbus-network` directly.

The server construction root preserves this order for each sibling:

```text
prepare and claim lease -> bind or consume prebound socket -> guard
  -> adopt and activate lease -> write observed projection -> spawn protocol
```

The main HTTP/WebSocket path either receives a CLI-prepared active lease or
adopts an externally supplied listener with external provenance. Router
preparation records logical HTTP/WebSocket listener observations only after it
knows the kernel address. The projection is rebuildable evidence. It is not a
lease, desired-state, or socket authority.

Standalone RESP is deliberately not a `WireProtocolAdapter`. `nimbus-kv` owns
its socket and protocol. Its listener adapter claims before bind. It adopts the
kernel address into an active lease and enforces loopback at bind and serve.
It settles only after a confirmed local close.

## Security And Protocol Matrix

| Surface | Socket/effect owner | Security guard | Protocol evidence |
| --- | --- | --- | --- |
| HTTP and WebSocket | `nimbus-server`, with the main bind retained by the CLI or external embedder | local-admin, application-tenant, origin, route-family, and WebSocket subprotocol gates remain in server/router owners | real `serve` tests, WebSocket negotiation/subscription tests, and full server suite |
| MongoDB | `nimbus-server` MongoDB adapter | unbound credentials stay loopback-only; bound credentials select one tenant and reject cross-tenant access | OP_MSG handshake, SCRAM, tenant admission, and cross-tenant rejection tests |
| DynamoDB | `nimbus-server` DynamoDB adapter | signature-skipping lookup mode stays loopback-only; strict signed mode remains the default | JSON-1.0 control-plane, item, query, transaction, and stream tests |
| S3 | `nimbus-server` S3 adapter | startup requires signed access keys; empty proxy secrets fail closed; unsigned requests fail | SigV4 presigned request, unsigned rejection, object placement, and download tests |
| RESP | `nimbus-kv` | loopback-only bind and serve plus exact tenant credential matching | RESP2/RESP3, auth isolation, lifecycle, contention, and fresh-process tests |

## Source-Derived Decisions

1. NNC7.1 is a verification-only integration item unless an acceptance test
   finds a current defect. The target production seam is already present.
2. Do not add a generic network listener provider. Server and KV keep socket,
   task, protocol, and security ownership.
3. Do not add lease methods to `WireProtocolAdapter`. The server composition
   root wraps every adapter with one shared listener-lease adapter.
4. Do not promote `nimbus-system` listener documents to desired state or
   allocation authority. NNC7.4 owns richer observed projections.
5. Do not add RESP to the server adapter list. Its standalone process and
   lifecycle stay `nimbus-kv`-owned.
6. Do not consume NNC7.1a. The existing kth-sibling failure stays as an
   expected-red test. The structured listener group owns atomic unwind,
   task-death propagation, and inherited-socket handling.
7. Do not change TLS authority. NNC7.6 retains certificate and interception-CA
   separation.

## Acceptance Ledger

| ID | Verifiable success criterion | Status |
| --- | --- | --- |
| P1 | The source census finds one private `WireProtocolAdapter` seam, exactly MongoDB/DynamoDB/S3 implementations, server-owned sibling binds, one main listener path, and one separate KV listener owner. | pass |
| P2 | Main and sibling paths prove claim/adopt/activate before serving, observed projection after activation, and confirmed-close settlement without using an address as identity. | pass |
| P3 | HTTP and WebSocket real-serve, route security, origin, tenant, and subprotocol tests pass. | pending |
| P4 | MongoDB adapter identity/guard plus real OP_MSG, SCRAM, tenant admission, and cross-tenant tests pass. | pending |
| P5 | DynamoDB adapter identity/guard plus all five deterministic protocol families pass. | pending |
| P6 | S3 adapter identity/guard, unsigned refusal, SigV4, placement, and download tests pass. | pending |
| P7 | RESP listener lifecycle, loopback refusal, tenant auth, RESP2/RESP3, contention, and fresh-process tests pass. | pending |
| P8 | The complete affected server and KV suites pass with exact counts and intentional ignores recorded. | pending |
| P9 | The source and dependency scan confirms that no socket, task, protocol, security, projection, or provider effect moved into `nimbus-network`; its only workspace edge remains `nimbus-core`. | pending |
| P10 | The NNC7.1a expected-red remains ignored in normal suites and reproduces only its named partial-start survivor defect when run explicitly. | pending |
| P11 | Affected check, strict Clippy/Rustdoc, format/diff, live/static verifier, proof lint, docs, and site gates pass. | pending |
| P12 | After P1-P11 pass, one GPT-5.6 Sol/xhigh/fast item review passes and the exact verification-only item is committed once. | pending |

## Verification Commands

Focused behavior:

```text
cargo test -p nimbus-server --lib adapters::wire::tests:: -- --test-threads=1
cargo test -p nimbus-server --lib construction::tests::nnc3_5_ -- --test-threads=1
cargo test -p nimbus-server --lib tests::websocket_protocol:: -- --test-threads=1
cargo test -p nimbus-server --lib tests::local_server_security:: -- --test-threads=1
cargo test -p nimbus-server --lib tests::mongodb_wire:: -- --test-threads=1
cargo test -p nimbus-server --lib tests::dynamodb_wire:: -- --test-threads=1
cargo test -p nimbus-server --lib adapters::s3::listener::tests:: -- --test-threads=1
cargo test -p nimbus-kv -- --test-threads=1
```

Expected-red boundary:

```text
cargo test -p nimbus-server --lib \
  construction::tests::nnc0_7_kth_adapter_failure_must_not_leave_prior_listener_live \
  -- --ignored --exact --nocapture
```

Final affected and quality gates:

```text
cargo test -p nimbus-server -- --test-threads=1
cargo test -p nimbus-kv -- --test-threads=1
cargo check --all-targets -p nimbus-server -p nimbus-kv
cargo clippy --all-targets -p nimbus-server -p nimbus-kv -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps -p nimbus-server -p nimbus-kv
cargo fmt --all --check
git diff --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

## Recovery Checkpoint

| Field | Value |
| --- | --- |
| Current item | NNC7.1 |
| Last durable commit | `fb9880e16b9b0a95768fe4f217318fa4532229aa` |
| Current owned paths | This proof and the canonical plan/index only. Product implementation has not started. |
| Last green | NNC6.1e2 item commit; NNC7.1 source census P1-P2. |
| Next action | Run the focused protocol/security matrix, then the one final affected and quality funnel if it remains green. |
| Blocker | none |
