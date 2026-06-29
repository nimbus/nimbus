# Cloudflare Adapters Operator Notes

Nimbus's Cloudflare adapter family is a primitives-first compatibility surface:
Workers, Workers KV, and Durable Objects are thin adapters over Nimbus runtime,
storage, engine, and service-substrate primitives. Nimbus does not embed
`workerd`.

## Startup

Cloudflare routes mount on the main Nimbus HTTP listener by default:

```text
nimbus start --app-dir ./app
```

Disable them with:

```text
nimbus start --no-cloudflare
```

When `--app-dir` contains `wrangler.jsonc`, `wrangler.json`, or
`wrangler.toml`, Nimbus parses these binding declarations:

- `kv_namespaces`
- `durable_objects.bindings`
- `d1_databases` (registered for follow-on work)
- `r2_buckets` (registered for follow-on work)

The startup summary reports the number of KV, Durable Object, D1, and R2
bindings mounted.

## Auth Posture

Cloudflare REST routes require generated dev credentials. The current credential
shape is the same access-key registry used by DynamoDB-compatible auth:

```http
Authorization: Bearer <ACCESS_KEY_ID>:<SECRET>
```

Each credential resolves to exactly one `TenantId`. Request paths, namespaces,
or Worker-supplied binding payloads never decide the tenant by themselves.

Generated credentials are a loopback-only single-tenant development convenience,
not production multi-tenant auth. Exposing Cloudflare routes on a non-loopback
listener requires the shared main-listener network opt-in; lifting this for a
production Cloudflare-compatible endpoint also requires:

- per-credential to tenant binding enforced at the Cloudflare adapter boundary;
- production credential provisioning, rotation, and revocation;
- TLS termination before any plaintext credential exchange.

Those production auth and secret-management pieces belong to the
service-identity / secret-management lane.

## Workers Runtime Boundary

The landed Workers runtime slice supports ES module default-export `fetch`:

```js
export default {
  async fetch(request, env, ctx) {
    return new Response("ok");
  },
};
```

Supported now:

| Surface | Status |
| --- | --- |
| `fetch(request, env, ctx)` dispatch | supported |
| `Request` basic URL/method/header/body reads | supported for buffered bodies |
| `Response` status/header/text-body return | supported for buffered bodies |
| `ctx.waitUntil` | supported and drained after response |
| `ctx.passThroughOnException` | recorded |
| KV namespace binding stubs (`env.NS`) | supported |
| Durable Object substrate | supported in server substrate; Worker front-door expansion is the next API layer |

Unsupported Worker APIs fail loudly with named errors; they must not silently
return `undefined` or no-op:

| Surface | Current behavior |
| --- | --- |
| `request.cf` | named unsupported error |
| `caches.default` | named unsupported error |
| KV `stream` reads | named unsupported error |
| full WHATWG streaming request/response bodies | named unsupported error |
| `scheduled()` triggers | unsupported follow-on |
| HTMLRewriter | unsupported follow-on |
| service bindings | unsupported follow-on |

The minimal runtime slice is enough to prove the KV wedge: a Worker whose
handler returns a `Response` and whose only binding I/O is Workers KV can run on
Nimbus.

## Workers KV

Workers KV is implemented over the `TenantKvStore` primitive from the
`nimbus-kv` NKV0 foundation work. The REST surface and Worker `env.NS` binding
share the same storage-key, metadata, TTL, and pagination mapping.

Supported:

- `get` with `text`, `json`, and `arrayBuffer` value coercion;
- `getWithMetadata`;
- `put` with `expiration` or `expirationTtl`;
- `delete`;
- `list` with prefix, limit, cursor, and `list_complete`;
- metadata capped to the Cloudflare KV-compatible size budget;
- key and value size validation.

## Durable Objects

The landed Durable Object substrate is keyed by:

```text
(tenant_id, do_namespace, do_id)
```

`tenant_id` is the isolation boundary. `idFromName`, `newUniqueId`, and
`idFromString` produce or validate the object id, but they do not carry tenant
authority. A forged 64-hex `idFromString` can only form a stub under the
authenticated caller's tenant and namespace.

The server substrate provides:

- per-DO activation leases with monotonically increasing `lease_epoch`;
- per-DO concurrency lanes, so two DOs in one tenant can make independent
  progress;
- per-instance storage transactions through the engine-backed
  `TenantKvStore::kv_apply_batch`;
- stale-epoch write rejection before queued output-gate messages are released;
- alarm state fenced to the current lease holder;
- WebSocket hibernation records with attachment and auto-response state.

### Cluster Handoff

Single-node Nimbus enforces one active holder for a DO id through the catalog
lease and per-instance lane. Cluster scale is a horizontal-scaling HS5
responsibility: placement and leasing must be per DO id beneath the tenant
ownership layer, not just per tenant. A tenant-wide lease would collapse all of
a tenant's DOs onto one node and defeat the scatter model.

### Transient Duplicate Contract

Nimbus on commodity infrastructure cannot honestly promise Cloudflare's
"globally one instance in the world" failure model under every ungraceful
failover. The contract is Orleans/Akka-style: a transient duplicate activation
window can exist during failover, and correctness rests on the per-DO storage
fence. Every write carries the activation lease epoch; a stale epoch is rejected
before commit and before queued output is released.

## Dependency Boundaries

- R2 is gated on the Nimbus object-storage/S3 surface (NOS Phase 3).
- D1 over SQLite/libSQL is an independent follow-on.
- Cluster-scale DO routing is gated on horizontal-scaling HS5 or a follow-on
  that adds per-DO-id placement and lease fencing.
- Full Workers-runtime API parity is a follow-on runtime-surface band.

## Deviation Register

### CF-DIV-001 - Workers KV consistency

**Cloudflare:** Workers KV is eventually consistent globally; writes can take up
to the documented propagation window to become visible at every edge.

**Nimbus:** reads are strongly consistent against the tenant's Nimbus storage
primitive.

**Rationale:** Nimbus is not reproducing edge propagation lag. Strong
consistency is a compatible safety upgrade for most local/self-hosted workloads:
clients see fresher data, never older data because Nimbus intentionally delayed
visibility.

**Regression evidence:** `cloudflare_worker_env_ns_e2e_round_trips_kv` writes
through `env.NS.put` and immediately reads through `env.NS.get`,
`getWithMetadata`, and `list`.

**Status:** accepted for CFA3/CFA5.

### CF-DIV-002 - Workers KV latency class

**Cloudflare:** Workers KV is an edge cache-class product optimized for
microsecond/local-edge reads in many deployments.

**Nimbus:** Workers KV rides the durable, strongly consistent `TenantKvStore`
path. It is a durable adapter surface, not a microsecond edge cache.

**Rationale:** This is the correct self-hosted primitive for consistency and
operator control. Operators must not market or tune it as a drop-in replacement
for Cloudflare's global edge read-latency profile.

**Regression evidence:** NKV0 and CFA tests prove the durable path and
immediate consistency; latency benchmarks are tracked by the `nimbus-kv` and
latency-budget lanes rather than the Cloudflare adapter verifier.

**Status:** accepted for CFA3/CFA5; revisit only if Nimbus adds an edge-cache
tier in front of `TenantKvStore`.

## Verification

Focused local checks:

```text
cargo test -p nimbus-server adapters::cloudflare -- --nocapture
cargo test -p nimbus-server durable_object -- --nocapture
cargo test -p nimbus-runtime cloudflare_workers -- --nocapture
cargo test -p nimbus-bin cloudflare_routes_refuse_non_loopback -- --nocapture
bash scripts/verify-cloudflare-adapters.sh
```

Closeout requires `bash scripts/verify-cloudflare-adapters.sh` to report
`12 passed, 0 failed` and a submitted PR for the active branch.
