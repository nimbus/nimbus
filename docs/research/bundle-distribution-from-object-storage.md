# Bundle Distribution from Object Storage

Research document for remote runtime bundle fetching and caching. This
feature enables multi-node Neovex deployments to pull V8 runtime bundles
(codegen output) from a central object store instead of requiring local
filesystem copies on every node.

**Status:** research
**Date:** 2026-04-02
**Depends on:** existing `ConvexRegistry::from_app_dir()` loading path
**Blocks:** multi-node deployment without shared filesystem

---

## Problem Statement

Today, `--app-dir` points at a local directory containing the codegen
output:

```
.neovex/convex/
  functions.json       # function manifest
  bundle.mjs           # ESM entrypoint
  bundle.sha256        # integrity hash
  http_routes.json     # optional HTTP route manifest
  auth.config.json     # optional auth provider config
  schema.json          # optional schema manifest
```

This works for single-node and local development. For multi-node production
deployments, operators must copy these files to every node via rsync, container
image rebuild, shared NFS mount, or similar. There is no built-in mechanism to
distribute bundles from a central store.

### Current Loading Path

```
neovex-bin/src/main.rs:96-103
  → ConvexRegistry::from_app_dir(path)
    → neovex-server/src/adapters/convex/registry/loading.rs:8-14
      → reads .neovex/convex/functions.json        (std::fs::read_to_string)
      → reads .neovex/convex/bundle.mjs            (load_runtime_bundle)
      → reads .neovex/convex/bundle.sha256         (std::fs::read_to_string)
      → reads .neovex/convex/http_routes.json      (optional)
      → reads .neovex/convex/auth.config.json      (optional)
      → reads .neovex/convex/schema.json           (optional)
```

All reads are synchronous `std::fs` calls at startup. The
`ConvexRegistry` is constructed once and shared across all tenants for the
lifetime of the server.

---

## Design Goals

1. **Preserve single-binary DX.** No external processes, FUSE mounts, or
   infrastructure dependencies beyond the object store itself.
2. **One flag to go remote.** `--app-dir s3://bucket/path` should work
   with the same flag that accepts local paths today. URI scheme detection
   selects the backend.
3. **Pre-warm is mandatory.** Bundles are fetched and verified before the server
   starts accepting connections. No request ever hits a cold cache.
4. **Reload without restart.** An API endpoint triggers re-fetch, verify, and
   hot-swap of the `ConvexRegistry`.
5. **Existing integrity model unchanged.** `RuntimeBundle::verify_integrity()`
   hashes the bundle before every V8 invocation. This remains the safety net
   regardless of how the bundle was fetched.

---

## Proposed CLI Surface

```bash
# Local development (unchanged)
neovex serve --app-dir ./my-app

# Single node, local bundles (unchanged)
neovex serve --app-dir /opt/bundles/my-app

# Remote bundles from S3
neovex serve --app-dir s3://my-bucket/my-app

# Remote bundles from GCS
neovex serve --app-dir gs://my-bucket/my-app

# Remote bundles from Azure Blob Storage
neovex serve --app-dir az://my-container/my-app

# Remote bundles from Cloudflare R2
neovex serve --app-dir s3://my-r2-bucket/my-app  # R2 is S3-compatible

# Explicit cache directory (optional, defaults to {data-dir}/bundle-cache)
neovex serve --app-dir s3://bucket/app --bundle-cache-dir /data/cache
```

Credentials follow standard cloud SDK conventions:

| Provider | Environment Variables |
|---|---|
| AWS S3 / R2 | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, `AWS_REGION`, `AWS_ENDPOINT_URL` (for R2) |
| GCS | `GOOGLE_APPLICATION_CREDENTIALS` (path to service account JSON) |
| Azure | `AZURE_STORAGE_ACCOUNT`, `AZURE_STORAGE_ACCESS_KEY` |

---

## Architecture

```
┌──────────────────────────────────────────────┐
│  neovex-bin (CLI)                            │
│  --app-dir s3://bucket/app                   │
└──────────────┬───────────────────────────────┘
               │ parse URI scheme
               ▼
┌──────────────────────────────────────────────┐
│  BundleSource (neovex-server)                │
│                                              │
│  enum BundleSourceConfig {                   │
│    Local { path: PathBuf },                  │
│    Remote {                                  │
│      url: String,                            │
│      cache_dir: PathBuf,                     │
│      store: Arc<dyn ObjectStore>,            │
│    },                                        │
│  }                                           │
│                                              │
│  ┌────────────────────────────────────────┐  │
│  │ resolve() → PathBuf                   │  │
│  │   Local: return path directly          │  │
│  │   Remote:                              │  │
│  │     1. check local cache + ETag        │  │
│  │     2. if stale: fetch from object     │  │
│  │        store via object_store crate    │  │
│  │     3. write to cache_dir              │  │
│  │     4. verify bundle SHA-256           │  │
│  │     5. return cache_dir path           │  │
│  └────────────────────────────────────────┘  │
└──────────────────┬───────────────────────────┘
                   │ local PathBuf
                   ▼
┌──────────────────────────────────────────────┐
│  ConvexRegistry::from_app_dir(local_path)    │
│  (unchanged — always sees local files)       │
└──────────────────────────────────────────────┘
```

### Cache Layout

```
{cache_dir}/
  convex/
    functions.json
    bundle.mjs
    bundle.sha256
    http_routes.json      # if present remotely
    auth.config.json      # if present remotely
    schema.json           # if present remotely
    .bundle-etag          # ETag from last successful fetch
    .bundle-fetched-at    # timestamp of last fetch
```

The cache directory mirrors the `.neovex/convex/` layout exactly so that
`ConvexRegistry::from_app_dir()` works unchanged.

### Reload Endpoint

```
POST /api/bundles/reload
```

1. Re-fetches from remote (or re-reads from local path).
2. Sends `If-None-Match` with cached ETag — skip download if unchanged.
3. Verifies `bundle.sha256` against fetched `bundle.mjs`.
4. Constructs a new `ConvexRegistry`.
5. Atomically swaps the `ConvexRegistry` in `AppState`.
6. Returns `200 OK` with `{ "reloaded": true, "sha256": "..." }` or
   `{ "reloaded": false, "reason": "unchanged" }`.

In-flight requests continue using the old registry. New requests pick up the
new one. No restart required.

---

## Key Dependency: `object_store` Crate

The Apache Arrow `object_store` crate is the Rust ecosystem standard for
cloud object storage access. It provides a unified `ObjectStore` trait across
S3, GCS, Azure, local filesystem, and in-memory (for tests).

### Why `object_store`

- Battle-tested: used by DataFusion, Delta Lake, InfluxDB IOx, crates.io
- Current version: 0.13.x (actively maintained, ~2-month release cadence)
- License: MIT + Apache 2.0
- Supports conditional operations (ETag-based `If-None-Match`)
- Feature-gated backends (only compile what you use)
- In-memory backend for deterministic testing
- Pure Rust, no FFI

### Key API Surface

```rust
// Core trait (simplified)
#[async_trait]
pub trait ObjectStore: Send + Sync + 'static {
    async fn get(&self, location: &Path) -> Result<GetResult>;
    async fn get_opts(&self, location: &Path, opts: GetOptions) -> Result<GetResult>;
    async fn put(&self, location: &Path, payload: PutPayload) -> Result<PutResult>;
    async fn head(&self, location: &Path) -> Result<ObjectMeta>;
    async fn delete(&self, location: &Path) -> Result<()>;
    async fn list(&self, prefix: Option<&Path>) -> BoxStream<Result<ObjectMeta>>;
}

// Conditional get (ETag-based)
let opts = GetOptions {
    if_none_match: Some(cached_etag),
    ..Default::default()
};
match store.get_opts(&path, opts).await {
    Ok(result) => { /* new data, update cache */ },
    Err(e) if e.to_string().contains("NotModified") => { /* cache is fresh */ },
    Err(e) => { /* real error */ },
}
```

### Alternative: OpenDAL

Apache OpenDAL (4.8k stars, 1.3M monthly downloads) provides a broader
abstraction layer with bindings for Python, Java, Go, Node.js, and C. It
supports 50+ backends (S3, GCS, Azure, Redis, RocksDB, PostgreSQL, HDFS,
and more) with built-in middleware layers for caching, retry, and metrics.
Used in production by GreptimeDB, Mozilla sccache, RisingWave, and Vector.
Pure Rust with its own HTTP implementations (no SDK wrappers).

External research argues OpenDAL is the stronger choice because of its
built-in caching layer and backend breadth. The counter-argument is that for
this narrow use case (fetch a handful of small files from object storage),
`object_store` is simpler and already the standard in the Rust data ecosystem.

| | `object_store` | OpenDAL |
|---|---|---|
| Focus | Cloud object storage | Universal data access |
| API style | Cloud-native (stateless, atomic) | Filesystem-like (read, write, stat, list) |
| Backends | S3, GCS, Azure, local, in-memory | 50+ services including object stores, KV, FS |
| Middleware | None built-in | Caching, retry, metrics, logging layers |
| Ecosystem | Arrow/DataFusion/Delta Lake | GreptimeDB, sccache, RisingWave, Databend |
| Binary impact | ~200KB per backend | Larger due to broader scope |
| Testing | In-memory backend built-in | In-memory backend built-in |
| Node.js bindings | No | Yes (relevant if JS SDK needs direct storage access) |

**Recommendation:** Start with `object_store` for bundle distribution. It is
the narrower, better-tested choice for this specific use case. If the runtime
file storage feature (separate research document) is built and needs richer
middleware (caching layers, retry, metrics), reconsider OpenDAL at that point
as the shared dependency for both features.

**Key decision factor:** If both features (bundle distribution AND runtime
file storage) end up sharing a single storage abstraction, OpenDAL becomes
the stronger choice because its middleware layers solve the read cache and
metrics problems that runtime file storage needs. If bundle distribution
ships first and alone, `object_store` is simpler.

---

## Implementation Estimate

| Component | Lines | Complexity |
|---|---|---|
| `BundleSource` trait + `LocalBundleSource` | ~50 | Trivial (passthrough) |
| `RemoteBundleSource` (fetch, cache, ETag, verify) | ~200 | Medium |
| CLI URI parsing + source construction | ~40 | Low |
| Reload API endpoint + `ConvexRegistry` hot-swap | ~80 | Medium |
| Tests (local, remote mock via `InMemory` backend) | ~200 | Medium |
| **Total** | **~570** | **Medium** |

### Files to Change

| File | Change |
|---|---|
| `crates/neovex-server/src/bundles/mod.rs` | New module: `BundleSource`, `LocalBundleSource`, `RemoteBundleSource` |
| `crates/neovex-server/src/bundles/remote.rs` | Object storage fetch, local cache, ETag tracking |
| `crates/neovex-server/src/adapters/convex/registry/loading.rs` | No change — receives resolved local path |
| `crates/neovex-server/src/router.rs` | Add `/api/bundles/reload` endpoint |
| `crates/neovex-server/src/state.rs` | `AppState` holds `BundleSource` + supports `ConvexRegistry` swap |
| `crates/neovex-bin/src/main.rs` | Parse URI scheme, construct `BundleSource`, resolve before registry |
| `crates/neovex-server/Cargo.toml` | Add `object_store` with feature flags for desired backends |
| `Cargo.toml` (workspace) | Add `object_store` to workspace dependencies |

### Cargo Dependencies

```toml
# In workspace Cargo.toml
[workspace.dependencies]
object_store = { version = "0.13", features = ["aws", "gcp", "azure"] }

# In neovex-server/Cargo.toml
[dependencies]
object_store = { workspace = true }
```

Feature flags are additive. Omit `gcp` or `azure` to reduce compile time
and binary size if only S3/R2 is needed initially.

---

## Alternative Approach: Custom `ModuleLoader`

External research surfaced a more deeply integrated alternative to the
whole-bundle-fetch model. Instead of fetching the entire bundle directory
upfront and pointing `ConvexRegistry::from_app_dir()` at a local cache,
implement a custom `deno_core::ModuleLoader` that resolves individual ESM
modules from the distributed store on demand.

### How It Would Work

Neovex already has `RestrictedModuleLoader` (module_loader.rs) which
restricts imports to the bundle root and uses `std::fs::read_to_string()`
to load module source. A distributed variant would:

1. On `resolve()`: validate the module specifier against the tenant's
   allowed root (same sandboxing as today).
2. On `load()`: check a local in-memory or disk LRU cache. On miss, fetch
   the single module file from object storage. Cache it locally.

```rust
// Sketch — not a proposal, just illustrating the integration point
impl ModuleLoader for DistributedModuleLoader {
    fn load(&self, specifier: &ModuleSpecifier, ...) -> ModuleLoadResponse {
        let path = specifier_to_storage_key(specifier);
        let source = self.cache.get_or_fetch(&path, || {
            self.store.get(&path)  // object_store / OpenDAL call
        });
        ModuleLoadResponse::Sync(Ok(ModuleSource::new(..., source, specifier)))
    }
}
```

### Tradeoffs vs Whole-Bundle Fetch

| | Whole-Bundle Fetch | Custom ModuleLoader |
|---|---|---|
| Startup latency | One bulk download at startup | Lazy — only fetch modules actually imported |
| Module count scaling | Download everything (even unused) | Fetch on demand |
| Integrity checking | SHA-256 of entire bundle | Per-module hashing (more complex) |
| Caching | Simple directory on disk | In-memory LRU + disk backing |
| Implementation | ~570 lines, medium | ~800 lines, higher complexity |
| ConvexRegistry changes | None | Needs manifest-first, bundle-lazy pattern |
| Offline resilience | Full bundle cached locally | May fail if cache cold + network down |

### Recommendation

Start with whole-bundle fetch (simpler, integrity model already exists).
The custom ModuleLoader approach is worth revisiting if:
- Bundle sizes grow large enough that full-download startup is slow
- Deployments have many tenants with distinct bundles (per-tenant bundles)
- Edge deployments need minimal cold-start latency

The two approaches are not mutually exclusive. The `BundleSource` could
pre-fetch the manifest and integrity hash, then lazy-load individual modules
via a custom ModuleLoader.

---

## Self-Hosted Backend: Garage

If operators want to self-host the object storage backend rather than using
a cloud provider, Garage is the strongest Rust-native option.

- **What:** S3-compatible distributed object storage, production since 2020
- **Language:** Rust
- **Architecture:** CRDT-based distributed state, no external coordination
  (no ZooKeeper/etcd). Runs on commodity hardware including Raspberry Pi.
- **License:** AGPL-3.0 (check compatibility with deployment model)
- **Link:** <https://garagehq.deuxfleurs.fr/>

Garage is relevant because it provides a self-hosted S3-compatible target
that `object_store` or OpenDAL can talk to without any code changes. For
operators who don't want cloud vendor dependency, the deploy model becomes:

```bash
# Self-hosted
neovex serve --app-dir s3://localhost:3900/bundles \
  --env AWS_ACCESS_KEY_ID=garage-key \
  --env AWS_SECRET_ACCESS_KEY=garage-secret
```

---

## Open Questions

### Q1: Should the reload endpoint require authentication?

The `/api/bundles/reload` endpoint triggers a re-fetch and hot-swap. In
production, this should probably require an admin token or be restricted to
localhost. The current Neovex HTTP API has no built-in auth for native routes.

**Options:**
- (a) Restrict to localhost by default, configurable via flag
- (b) Require a `--admin-token` that must be passed as a Bearer token
- (c) Defer to external reverse proxy (nginx, Caddy) for auth

### Q2: Should bundle fetching support per-tenant bundles?

Today, one `ConvexRegistry` serves all tenants. The Convex model is one app =
one bundle. Per-tenant bundles would be a significant model change requiring
separate V8 bundles, manifests, and schemas per tenant.

**Current recommendation:** No. Keep one bundle per server. Per-tenant bundles
are a separate feature that requires its own design (tenant → bundle mapping,
per-tenant registry, per-tenant hot reload, etc.).

### Q3: Should we support push-based reload (webhooks)?

Instead of the operator calling `POST /api/bundles/reload`, the server could
accept a webhook from CI/CD or S3 event notifications.

**Current recommendation:** Start with pull-based (explicit reload endpoint).
Push-based can be layered on top without architectural changes — the webhook
handler just calls the same reload logic.

### Q4: What happens during reload if V8 invocations are in flight?

The `ConvexRegistry` is behind an `Arc`. Swapping it means new requests get
the new registry; in-flight requests keep their reference to the old one.
`RuntimeBundle::verify_integrity()` hashes the file on disk, so if the cache
directory is overwritten mid-invocation, the hash check could fail.

**Mitigation options:**
- (a) Write new cache to a temp directory, then atomic rename
- (b) Use versioned cache directories (`cache/v1/`, `cache/v2/`)
- (c) The existing SHA-256 check catches any inconsistency — the invocation
  fails and retries against the new bundle

### Q5: How does this interact with the license file?

The license file (`--license-file` or `.neovex/license.json`) is loaded once
at startup and is not part of the bundle. It should remain separate from
bundle distribution. No change needed.

### Q6: Should we poll for changes on a timer?

Instead of (or in addition to) the reload endpoint, the server could
periodically check the remote ETag and reload if changed.

**Current recommendation:** No. Explicit reload is simpler, more predictable,
and avoids polling overhead. Polling can be added later if operators request
it.

---

## Deploy Workflow (Expected)

```bash
# 1. Developer builds the bundle
npx @neovex/codegen --app ./my-app --output .neovex/convex/

# 2. Upload to object storage
aws s3 sync .neovex/convex/ s3://my-bucket/my-app/.neovex/convex/

# 3. Tell Neovex nodes to reload
curl -X POST http://neovex-node-1:8080/api/bundles/reload
curl -X POST http://neovex-node-2:8080/api/bundles/reload

# Or from CI/CD:
for node in $NEOVEX_NODES; do
  curl -X POST http://$node:8080/api/bundles/reload
done
```

This mirrors how most production systems handle deployment artifact
distribution — push to central store, notify consumers.

---

## References

- `object_store` crate: <https://docs.rs/object_store/latest/object_store/>
- `object_store` GitHub: <https://github.com/apache/arrow-rs-object-store>
- OpenDAL: <https://github.com/apache/opendal>
- Current bundle loading: `crates/neovex-server/src/adapters/convex/registry/loading.rs`
- Current CLI: `crates/neovex-bin/src/main.rs`
- `ConvexRegistry` struct: `crates/neovex-server/src/adapters/convex/mod.rs:55-63`
- `RuntimeBundle` integrity: `crates/neovex-runtime/src/runtime.rs:283-299`
- `RestrictedModuleLoader`: `crates/neovex-runtime/src/module_loader.rs`
