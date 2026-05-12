# Runtime File Storage Surface

Research document for exposing a tenant-scoped filesystem to JavaScript
functions running in the V8 runtime. This feature gives `ctx.fs` operations
to Nimbus actions and mutations, backed by local disk or object storage.

**Status:** research
**Date:** 2026-04-02
**Depends on:** HostBridge operation model, tenant isolation model
**Related:** `bundle-distribution-from-object-storage.md` (separate feature,
shares the `object_store` dependency)

---

## Problem Statement

Nimbus's V8 runtime currently exposes database operations (`ctx.db.*`),
scheduling (`ctx.scheduler.*`), and nested function calls (`ctx.runQuery`,
`ctx.runMutation`, `ctx.runAction`). There is no file or blob storage surface.

JavaScript functions that need to read configuration files, process uploads,
generate exports, or work with binary assets have no built-in mechanism. The
workarounds are:

- Store file contents as base64 in document fields (size limits, inefficient)
- Require an external storage service and make HTTP calls from actions
  (breaks the single-binary convenience model)
- Mount an external filesystem into the process (breaks single-binary DX)

### What This Feature Enables

- User uploads a file → stored durably → JS function processes it
- JS function generates a report/export → writes to storage → user downloads
- Configuration files, templates, or assets shared across function invocations
- ML model weights or large reference data accessible from functions
- Per-tenant file isolation matching the per-tenant database isolation

---

## Design Space

There are three broad approaches. This section evaluates each against
Nimbus's architecture constraints.

### Approach A: Bespoke Blob API (`ctx.storage`)

The Convex model. Opaque storage IDs, no paths, no directories.

```javascript
// Convex-style
const blob = await ctx.storage.get(storageId);
const url = await ctx.storage.getUrl(storageId);
const newId = await ctx.storage.store(processedBlob);
```

**Pros:**
- Minimal API surface
- Tenant isolation is trivial (IDs are opaque, scoped to tenant)
- Matches Convex compatibility surface

**Cons:**
- No directory listing, no path-based organization
- Developers must maintain their own path → ID mapping in documents
- Not natural for JS/TS developers used to filesystem patterns
- Every platform that started with blob IDs later added path support

**Verdict:** Too limited for a Nimbus-native feature. May be worth
implementing for Convex compatibility, but not as the primary file storage
surface.

### Approach B: Virtual Filesystem via Host Operations (`ctx.fs`)

Expose filesystem semantics through the existing HostBridge pattern. Paths
are virtual and tenant-scoped. The backend can be local disk or object
storage. No FUSE mount needed.

```javascript
// Nimbus-native
const config = await ctx.fs.readFile("/config/settings.json", "utf8");
await ctx.fs.writeFile("/exports/report.csv", csvData);
const entries = await ctx.fs.readDir("/uploads/");
const exists = await ctx.fs.exists("/templates/email.html");
await ctx.fs.delete("/temp/working.json");
const meta = await ctx.fs.stat("/uploads/image.png");
```

**Pros:**
- Familiar API for JS/TS developers
- Hierarchical path organization
- Directory listing and globbing
- Tenant isolation enforced by HostBridge (not filesystem permissions)
- No external dependencies (single-binary preserved)
- Backend-agnostic (local disk for dev, object storage for prod)

**Cons:**
- Larger API surface than blob storage
- Must define consistency model (read-after-write? listing consistency?)
- Path validation and sandboxing required (no `../` escapes)

**Verdict:** Best fit for Nimbus's DX goals. Filesystem semantics through
HostBridge operations. This is the recommended approach.

### Approach C: Native `node:fs` via FUSE Mount

Mount a real filesystem (JuiceFS, ZeroFS, etc.) and let V8 use standard
Node.js `fs` APIs.

```javascript
import { readFile } from 'node:fs/promises';
const data = await readFile('/tenant-fs/config.json', 'utf8');
```

**Pros:**
- Zero API to learn — standard Node.js
- Maximum compatibility with existing Node.js code

**Cons:**
- Breaks single-binary model (requires FUSE mount + metadata engine)
- Security: `deno_core` sandbox explicitly prevents real `fs` syscalls
- Tenant isolation via filesystem permissions is fragile
- Per-tenant FUSE mounts are operationally complex
- Privileged containers required for FUSE in Docker/Kubernetes
- Cannot audit or rate-limit file operations at the application level

**Verdict:** Not viable without fundamental changes to the runtime sandbox
and deployment model. The security and operational costs outweigh the
familiarity benefit.

---

## Recommended Design: `ctx.fs` Host Operations

### JavaScript API Surface

```javascript
// Reading
ctx.fs.readFile(path)                 // → ArrayBuffer
ctx.fs.readFile(path, "utf8")         // → string
ctx.fs.readFile(path, "base64")       // → base64 string

// Writing
ctx.fs.writeFile(path, data)          // data: string | ArrayBuffer | Uint8Array
ctx.fs.writeFile(path, data, opts)    // opts: { contentType?: string }

// Metadata
ctx.fs.stat(path)                     // → { size, contentType, modified, created }
ctx.fs.exists(path)                   // → boolean

// Directory operations
ctx.fs.readDir(path)                  // → [{ name, type: "file"|"dir", size }]
ctx.fs.readDir(path, { recursive })   // → [{ name, type, size, path }]

// Deletion
ctx.fs.delete(path)                   // → void (idempotent)

// URL generation (for client downloads/uploads)
ctx.fs.getUrl(path)                   // → signed URL string
ctx.fs.getUploadUrl(path)             // → signed upload URL string
```

**Path rules:**
- All paths are relative to the tenant root. Leading `/` is optional.
- No `..` components allowed. Validated before any backend operation.
- Paths are UTF-8 strings. Forward slashes only.
- Maximum path length: 1024 characters.
- Maximum path depth: 32 components.

**Not included in initial surface:**
- `copy()` / `move()` — can be added later
- `glob()` — can be built on `readDir` with recursive option
- Streaming read/write — initial version reads/writes full files
- File locking — not needed for the HostBridge model (functions are
  short-lived)
- Symlinks — not applicable for virtual filesystem

### Host Operations

New `deno_core` ops in `nimbus-runtime`:

```
op_nimbus_ctx_fs_read          async   HostCallRequest { operation: "fs.read", payload: { path, encoding? } }
op_nimbus_ctx_fs_write         async   HostCallRequest { operation: "fs.write", payload: { path, data, content_type? } }
op_nimbus_ctx_fs_stat          async   HostCallRequest { operation: "fs.stat", payload: { path } }
op_nimbus_ctx_fs_exists        async   HostCallRequest { operation: "fs.exists", payload: { path } }
op_nimbus_ctx_fs_read_dir      async   HostCallRequest { operation: "fs.readDir", payload: { path, recursive? } }
op_nimbus_ctx_fs_delete        async   HostCallRequest { operation: "fs.delete", payload: { path } }
op_nimbus_ctx_fs_get_url       async   HostCallRequest { operation: "fs.getUrl", payload: { path, expires_secs? } }
op_nimbus_ctx_fs_get_upload_url async  HostCallRequest { operation: "fs.getUploadUrl", payload: { path, expires_secs? } }
```

These follow the exact same pattern as existing `op_nimbus_ctx_db_*` and
`op_nimbus_ctx_scheduler_*` operations. The runtime crate defines the ops
and routes them through `HostCallRequest`. The server's `ConvexHostBridge`
implementation handles the actual storage interaction.

### Architecture

```
┌──────────────────────────────────────────────────────┐
│  JavaScript (V8)                                     │
│  await ctx.fs.readFile("/reports/q1.csv", "utf8")    │
│    │                                                 │
│    ▼                                                 │
│  op_nimbus_ctx_fs_read → HostCallRequest {           │
│    operation: "fs.read",                             │
│    payload: { path: "/reports/q1.csv", encoding: "utf8" }  │
│  }                                                   │
└──────────────────────────┬───────────────────────────┘
                           │
              HostBridge (nimbus-runtime boundary)
              (runtime has zero workspace deps)
                           │
                           ▼
┌──────────────────────────────────────────────────────┐
│  ConvexHostBridge (nimbus-server)                    │
│                                                      │
│  match "fs.read" →                                   │
│    validate_path(path)?                              │
│    file_store.read(tenant_id, path).await?           │
│    encode response (utf8 / base64 / raw)             │
└──────────────────────────┬───────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────┐
│  TenantFileStore                                     │
│  (nimbus-engine or nimbus-server)                    │
│                                                      │
│  trait FileStoreBackend: Send + Sync + 'static {     │
│    async fn read(&self, key: &str) -> Result<Bytes>; │
│    async fn write(&self, key: &str, data: Bytes,     │
│                   ct: Option<&str>) -> Result<()>;   │
│    async fn stat(&self, key: &str)                   │
│                  -> Result<Option<FileMeta>>;         │
│    async fn delete(&self, key: &str) -> Result<()>;  │
│    async fn list(&self, prefix: &str)                │
│                  -> Result<Vec<FileEntry>>;           │
│    async fn signed_read_url(&self, key: &str,        │
│                  expires: Duration) -> Result<String>;│
│    async fn signed_write_url(&self, key: &str,       │
│                  expires: Duration) -> Result<String>;│
│  }                                                   │
│                                                      │
│  ┌──────────────────┐  ┌────────────────────────┐    │
│  │ LocalFsBackend   │  │ ObjectStoreBackend     │    │
│  │ {data-dir}/files │  │ object_store crate     │    │
│  │  /{tenant_id}/   │  │ + optional local read  │    │
│  │                  │  │   cache                 │    │
│  └──────────────────┘  └────────────────────────┘    │
└──────────────────────────────────────────────────────┘
```

### Key Design Decisions

**Tenant scoping.** Every file operation is prefixed with the tenant ID.
`ctx.fs.readFile("/config.json")` in tenant `t1` maps to:
- Local: `{data-dir}/files/t1/config.json`
- Remote: `s3://bucket/t1/config.json`

The tenant ID prefix is added by the HostBridge, not by the JS code. JS
code never sees or controls the tenant boundary.

**Path validation.** Before any backend call:
1. Reject paths containing `..`
2. Normalize `/./` and `//`
3. Reject paths longer than 1024 chars or deeper than 32 components
4. Reject null bytes and non-UTF-8

**Consistency model.**
- **Read-after-write within the same tenant:** guaranteed. A write followed
  by a read in the same or subsequent function invocation always sees the
  written data.
- **Cross-tenant:** no visibility. Tenants cannot see each other's files.
- **Listing consistency:** best-effort. A write followed by a `readDir` may
  not immediately include the new file on object storage backends (S3 has
  strong listing consistency since 2020; GCS has always had it).

**No transactional integration with document mutations.** File writes are NOT
part of the redb transaction that commits document changes. A mutation that
writes a file and then fails will leave the file written. This is the same
model as Convex `ctx.storage` and every other serverless file storage API.

**Size limits.** Configurable per deployment:
- Default maximum file size: 100 MB per file
- Default maximum total storage per tenant: 10 GB
- Enforced at the HostBridge level before the backend call

---

## Backend Options

### Local Filesystem Backend

```rust
struct LocalFsBackend {
    root: PathBuf,  // {data-dir}/files/
}
```

Files stored at `{root}/{tenant_id}/{path}`. Standard `tokio::fs` operations.
Directories created on demand. This is the default for local development
and single-node deployments.

**Advantages:** Zero configuration. Works with the current single-binary
model. Files are on local NVMe alongside redb tenant databases.

**Disadvantages:** Files are not replicated. Lost if the disk fails. Not
suitable for multi-node deployments without shared storage.

### Object Storage Backend (`object_store`)

```rust
struct ObjectStoreBackend {
    store: Arc<dyn ObjectStore>,
    read_cache: Option<ReadCache>,  // optional local disk cache
}
```

Files stored at `{prefix}/{tenant_id}/{path}` in the configured object store.
Uses the same `object_store` crate as bundle distribution.

**Advantages:** Durable, replicated, multi-node. Standard cloud storage.

**Disadvantages:** Higher latency for reads (mitigated by optional local
cache). Requires cloud credentials.

### Optional Local Read Cache

For the object storage backend, an optional local disk cache reduces read
latency for frequently accessed files:

```rust
struct ReadCache {
    cache_dir: PathBuf,      // local disk cache location
    max_size_bytes: u64,     // total cache size limit
    // LRU eviction when cache is full
}
```

**Cache behavior:**
- Read hit: serve from local disk (~0.1ms)
- Read miss: fetch from object storage (~50-200ms), cache locally, serve
- Write: write-through (write to object storage, then cache locally)
- Invalidation: on delete, evict from cache. On write, replace cache entry.
- Eviction: LRU when total cache size exceeds limit.

The cache is a performance optimization, not a consistency mechanism. The
object store is always the source of truth.

An alternative cache design from the `rust-vfs` crate is the **OverlayFS
pattern**: layer a remote-backed filesystem under a local write cache. Reads
check the local layer first, then fall through to remote. Writes go to both
layers. This is architecturally similar to the `ReadCache` above but framed
as a composable filesystem layer rather than a bespoke cache.

### Self-Hosted Backend: Garage

For operators who want to self-host rather than use a cloud provider,
Garage is a Rust-native S3-compatible distributed object storage system.
Production since 2020, CRDT-based (no ZooKeeper/etcd), runs on commodity
hardware. AGPL-3.0 licensed. See the bundle distribution research doc for
details. Both features would use the same self-hosted backend.

### Storage Abstraction: `object_store` vs OpenDAL

Both features (bundle distribution and runtime file storage) need an object
storage abstraction. The choice between `object_store` and OpenDAL is more
consequential for this feature than for bundle distribution because runtime
file storage benefits from OpenDAL's built-in middleware:

| Need | `object_store` | OpenDAL |
|---|---|---|
| Read cache layer | Build yourself | Built-in `CacheLayer` middleware |
| Retry on failure | Build yourself | Built-in `RetryLayer` middleware |
| Metrics/tracing | Build yourself | Built-in `MetricsLayer`, `LoggingLayer` |
| Rate limiting | Build yourself | Built-in `ThrottleLayer` |

If both features share one dependency, OpenDAL is the stronger choice
because its middleware layers directly solve the caching, retry, and
observability needs of runtime file storage. If bundle distribution ships
first alone, start with `object_store` and reconsider when this feature
begins implementation.

---

## Rust VFS and Trait Ecosystem

Several Rust crates provide virtual filesystem abstractions that could
inform or underpin the `FileStoreBackend` trait design.

### `sys_traits` (Deno ecosystem)

From David Sherret (Deno core team). Provides trait-per-function abstractions
for system operations with `RealSys` for actual OS calls and `InMemorySys`
for testing. Minimal and composable — individual traits like `BaseFsWrite`
and `BaseFsRead` rather than one monolithic `FileSystem` trait.

**Relevance:** Most naturally aligned with `deno_core` since it comes from
the same ecosystem. If we want our `FileStoreBackend` to be testable with
in-memory implementations (which we do), this crate's design pattern is
worth studying.

- **Link:** <https://lib.rs/crates/sys_traits>

### `virtual_fs` (Wasmer)

Wasmer's `FileSystem` trait includes `readlink`, `read_dir`, `create_dir`,
`remove_dir`, `rename`, `metadata`, `symlink_metadata`, `remove_file`,
`new_open_options`, and `mount`. Includes memory FS, union FS, and chained
filesystem implementations.

**Relevance:** Most feature-complete VFS trait in the Rust ecosystem. Built
specifically for sandboxing runtime environments (WASI), which is
architecturally very similar to what we're doing with `deno_core`. The
union/chained FS concepts map to the local-cache-over-remote pattern.

- **Link:** <https://docs.rs/virtual-fs/latest/virtual_fs/>

### `rust-vfs`

Provides `PhysicalFS`, `MemoryFS`, `AltrootFS`, `OverlayFS`, and
`EmbeddedFS` implementations with an async port behind a feature flag.

**Relevance:** The `OverlayFS` abstraction (layer a remote-backed FS under
a local write cache) is directly relevant to our read cache design. The
`AltrootFS` (jail a filesystem to a subdirectory) maps to our tenant
scoping. Good for inspiration but likely too simple for production use —
no symlinks, basic trait surface.

- **Link:** <https://github.com/manuel-wober/rust-vfs>

### Design Recommendation

Don't depend on any of these crates directly. Instead, study their trait
designs and build a minimal `FileStoreBackend` trait that:

1. Follows `sys_traits`' pattern of composable per-operation traits
   (or at minimum, keep the trait surface small and focused)
2. Supports the `rust-vfs` OverlayFS pattern for read caching
3. Has an in-memory implementation for deterministic testing
   (following both `sys_traits` and `rust-vfs` precedent)
4. Is async-native (unlike `rust-vfs` which is sync-first with async behind
   a feature flag)

---

## FUSE and Kernel-Level Mount Options

These are not recommended for the initial `ctx.fs` implementation but are
documented here for completeness. They become relevant if Nimbus ever needs
to expose tenant files outside the V8 runtime (NFS access, sidecar
containers, operator tooling).

### `fuser`

Rust rewrite of the FUSE C library. Production-quality, actively maintained.
The standard choice for building custom FUSE filesystems in Rust.

- **Link:** <https://github.com/cberner/fuser>

### `libfuse-fs`

Async-native FUSE crate supporting async trait interfaces, multi-core
optimization, unprivileged mounts, readdirplus, and POSIX locks.
Architecturally more aligned with async Rust and tokio than `fuser`.

- **Link:** <https://r2cn.dev/p/libfuse-fs>

### `ofs` (OpenDAL File System)

FUSE mount backed by OpenDAL. Lets you FUSE-mount any OpenDAL-supported
backend as a local filesystem. Limitation: OpenDAL does not support random
writes, so `ofs` has limited write support. Suitable for read-heavy use
cases like JS bundle serving.

- **Link:** <https://github.com/apache/opendal/tree/main/bin/ofs>

### When These Become Relevant

If Nimbus needs to expose tenant files via NFS, SFTP, or a block device
(for sidecar containers, backup tools, or IDE integration), the
`FileStoreBackend` trait provides the abstraction point. A `fuser`-based
or `ofs`-based layer could expose the same storage through a kernel mount
while the V8 runtime continues using `ctx.fs` host operations. ZeroFS
(NFS/9P/NBD over object storage, Rust-native, 1.7k stars) is the most
complete reference for this path.

---

## Comparison: How Other Platforms Do This

### Convex `ctx.storage`

- **Model:** Opaque ID-based blob storage. No paths, no directories.
- **API:** `get(id)`, `getUrl(id)`, `generateUploadUrl()`, `store(blob)`, `delete(id)`
- **Client upload pattern:** Generate short-lived URL → client uploads directly → function receives storage ID
- **Consistency:** Immediate within function context
- **Integration:** `ctx.storage` injected in mutations and actions

**Takeaway:** The upload URL pattern is worth adopting. It avoids file
data flowing through the function and reduces bandwidth costs.

### Cloudflare R2 (Workers Binding)

- **Model:** Flat key-value with prefix-based listing. S3-compatible.
- **API:** `put(key, body)`, `get(key)`, `head(key)`, `delete(key)`, `list(opts)`
- **Consistency:** Strong read-after-write (unique among object stores)
- **Conditional ops:** `ifMatch`, `ifNoneMatch` on ETags
- **Multipart:** Supported for large files
- **Integration:** Binding in `wrangler.toml`, accessed via `env.R2_BUCKET`

**Takeaway:** Strong consistency and conditional operations via ETags are
worth considering. R2's pricing (free egress) makes it an attractive
backend for Nimbus deployments on Cloudflare.

### Vercel Blob

- **Model:** Hierarchical paths with prefix-based listing.
- **API:** `put(path, body)`, `get(path)`, `head(path)`, `del(path)`, `list(opts)`, `copy(from, to)`
- **Consistency:** ETag-based conditional operations. CDN cache ~60s.
- **Integration:** `import { put, get } from '@vercel/blob'` + env token

**Takeaway:** Hierarchical paths with prefix listing is the right model.
Their approach of treating blobs as immutable (create new, don't update)
is a useful convention to document.

### Supabase Storage

- **Model:** Bucket-based with hierarchical paths. S3-compatible.
- **API:** `upload(path, file)`, `download(path)`, `remove([paths])`, `move(from, to)`, `createSignedUrl(path, expiresIn)`
- **Consistency:** Atomic uploads, eventual metadata
- **Resumable uploads:** TUS protocol for large files
- **Integration:** `supabase.storage.from('bucket').upload(...)`

**Takeaway:** The bucket concept maps to tenants. Signed URL generation
for both reads and writes is a good pattern.

### Firebase Cloud Storage

- **Model:** Hierarchical paths with reference-based API.
- **API:** `ref(storage, path)` → `uploadBytes(ref, data)`, `getDownloadURL(ref)`, `deleteObject(ref)`, `list(ref)`, `getMetadata(ref)`
- **Metadata:** First-class (contentType, cacheControl, custom properties)
- **Integration:** `import { getStorage, ref } from 'firebase/storage'`

**Takeaway:** First-class metadata (contentType, custom properties) is
useful. The reference-based API pattern is interesting but doesn't fit
the HostBridge model well.

### Summary Table

| Platform | Path Model | Consistency | Upload Pattern | Signed URLs |
|---|---|---|---|---|
| Convex | Opaque IDs | Immediate | Upload URL → client | `getUrl()` |
| R2 Workers | Flat keys | Strong R-A-W | Direct in function | N/A (binding) |
| Vercel Blob | Hierarchical | ETag conditional | Direct or client | Public URLs |
| Supabase | Bucket + path | Atomic upload | Direct or TUS | `createSignedUrl()` |
| Firebase | Hierarchical | Atomic upload | Client SDK | `getDownloadURL()` |
| **Nimbus (proposed)** | **Hierarchical** | **Strong R-A-W** | **Direct + upload URL** | **`getUrl()` + `getUploadUrl()`** |

---

## Filesystem-over-Object-Storage Landscape

These projects were evaluated for potential use as the storage backend or
as reference implementations. None are recommended as direct dependencies
for the `ctx.fs` feature, but they inform design decisions.

### Evaluated Projects

| Project | Type | Language | POSIX | Embeddable? | Maintained | Stars |
|---|---|---|---|---|---|---|
| JuiceFS | FS over object storage | Go | Excellent (8,813 tests) | No (FUSE daemon) | Active | 13.3k |
| ZeroFS | FS over object storage | Rust | Excellent (8,662 tests) | Yes (crate) | Active | 1.7k |
| OpenDAL | Storage abstraction | Rust | Partial (via `ofs`) | Yes (crate, 1.3M/mo) | Active | 4.8k |
| RustFS | Object storage server | Rust | N/A | Yes | Active | 24.1k |
| Kiseki | JuiceFS Rust port | Rust | No (learning project) | No | Minimal | 33 |
| Curvine | Cache system | Rust | Partial | No (source only) | Active | 616 |
| s3fs-fuse | S3 FUSE mount | C++ | Subset | No (FUSE daemon) | Active | 9.8k |
| goofys | S3 FUSE mount | Go | Partial (by design) | No (FUSE daemon) | Unmaintained | 5.5k |

### Key Observations

**JuiceFS** is the gold standard for FUSE-based filesystem-over-object-storage
but requires Go, Redis/TiKV, and a FUSE mount. Not embeddable in a Rust
binary. Useful as a reference for caching strategy and metadata design.

**ZeroFS** is the most interesting Rust-native option. It passes 8,662
pjdfstest_nfs tests, uses SlateDB LSM + S3, supports NFS/9P/NBD protocols
(not just FUSE), and is available as a crate. If Nimbus ever needs to expose
tenant files via NFS or a block device, ZeroFS is the reference. However, for
`ctx.fs` host operations, using `object_store` directly is simpler because we
don't need POSIX semantics — we control the entire API surface.

**OpenDAL** is a strong alternative to `object_store` with broader backend
support (60+ services). Its `ofs` subproject provides POSIX access. The
Node.js bindings could be useful if the JS SDK ever needs direct storage
access outside the runtime. However, for the initial implementation,
`object_store` is a better fit because it's narrower and already adopted
for bundle distribution.

**RustFS** is an object storage server (MinIO alternative), not a filesystem
layer. Not relevant for this feature but could be relevant if Nimbus ever
needs to embed an S3-compatible storage server.

### When FUSE/NFS Becomes Relevant

The `ctx.fs` HostBridge approach handles the V8 runtime use case. But there
are scenarios where a real filesystem mount would add value:

- Exposing tenant files to non-Nimbus tools (backup scripts, analytics)
- Mounting tenant files into sidecar containers
- SFTP/WebDAV access for operators
- IDE integration for file browsing

If these become requirements, the `FileStoreBackend` trait provides the
abstraction point. A ZeroFS-backed or JuiceFS-backed implementation could
expose the same files via NFS/FUSE while the V8 runtime continues using
`ctx.fs` host operations.

---

## CLI Surface

```bash
# Local development — files on local disk (default)
nimbus serve --data-dir ./data
# Files at ./data/files/{tenant_id}/

# Production — files in object storage
nimbus serve --data-dir /data --file-store s3://my-bucket/tenant-files
# Files at s3://my-bucket/tenant-files/{tenant_id}/

# With local read cache for object storage backend
nimbus serve --data-dir /data \
  --file-store s3://my-bucket/tenant-files \
  --file-cache-dir /data/file-cache \
  --file-cache-max-gb 10
```

When `--file-store` is not specified, files are stored locally at
`{data-dir}/files/`. This requires zero configuration for development.

---

## Implementation Estimate

### Phase 1: Core `ctx.fs` with Local Backend

| Component | Lines | Complexity |
|---|---|---|
| Runtime ops (6-8 new ops in `runtime.rs`) | ~150 | Low (follows existing pattern) |
| JS bootstrap (`ctx.fs` object in bootstrap JS) | ~80 | Low |
| `FileStoreBackend` trait | ~40 | Low |
| `LocalFsBackend` implementation | ~150 | Low-Medium |
| Path validation and sandboxing | ~60 | Low |
| HostBridge integration (match `fs.*` operations) | ~100 | Medium |
| Engine-level `TenantFileStore` (tenant scoping) | ~80 | Low |
| Tests | ~300 | Medium |
| **Phase 1 total** | **~960** | **Medium** |

### Phase 2: Object Storage Backend + Read Cache

| Component | Lines | Complexity |
|---|---|---|
| `ObjectStoreBackend` implementation | ~200 | Medium |
| `ReadCache` (LRU, disk-backed) | ~200 | Medium |
| Signed URL generation (read + write) | ~100 | Medium |
| CLI flags and configuration | ~40 | Low |
| Tests (mock with `InMemory` backend) | ~200 | Medium |
| **Phase 2 total** | **~740** | **Medium** |

### Phase 3: Convex Compatibility (`ctx.storage`)

| Component | Lines | Complexity |
|---|---|---|
| Convex `ctx.storage` API surface (compatibility shim) | ~150 | Low |
| Upload URL generation endpoint | ~80 | Low |
| `_storage` system table integration | ~100 | Medium |
| Tests | ~150 | Medium |
| **Phase 3 total** | **~480** | **Medium** |

### Files to Change

| File | Change |
|---|---|
| `crates/nimbus-runtime/src/runtime.rs` | Add `op_nimbus_ctx_fs_*` ops and bootstrap JS |
| `crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs` | Handle `fs.*` operations |
| `crates/nimbus-server/src/files/mod.rs` | New module: `FileStoreBackend` trait, `TenantFileStore` |
| `crates/nimbus-server/src/files/local.rs` | `LocalFsBackend` |
| `crates/nimbus-server/src/files/object_store.rs` | `ObjectStoreBackend` (Phase 2) |
| `crates/nimbus-server/src/files/cache.rs` | `ReadCache` (Phase 2) |
| `crates/nimbus-engine/src/tenant.rs` | Tenant-level file store reference |
| `crates/nimbus-bin/src/main.rs` | `--file-store` and `--file-cache-*` flags |
| `ARCHITECTURE.md` | Document the file storage surface and invariants |

---

## Open Questions

### Q1: Should file writes be part of the mutation execution unit?

Currently, document writes within a mutation are staged and committed
atomically. Should file writes participate in this transaction?

**Arguments for:** Consistency — a mutation that writes a document and a file
either both commit or neither does.

**Arguments against:** Object storage writes are not transactional. You can't
roll back an S3 PUT. The local filesystem backend could support this, but it
would create a behavioral difference between backends. Every other serverless
platform (Convex, Supabase, Firebase) explicitly does NOT include file writes
in the mutation transaction.

**Recommendation:** No. File writes are side effects, like sending an email.
Document this clearly. If atomic file-and-document operations are needed,
users should use actions (which already have non-transactional semantics).

### Q2: Should `ctx.fs` be available in queries?

Queries are read-only and re-evaluated for subscriptions. If `ctx.fs.readFile`
is allowed in queries, file reads become part of the subscription dependency
set. A file change would need to invalidate subscriptions that read it.

**Arguments for:** Read-only access in queries is conceptually clean.

**Arguments against:** File change detection for subscription invalidation is
complex. Files don't have the same commit/sequence model as documents. The
dependency tracking system would need a new dependency type.

**Recommendation:** Initially, `ctx.fs` is available only in actions and
mutations (same as Convex's `ctx.storage`). Queries cannot access files.
This avoids the subscription invalidation problem entirely.

### Q3: Where should `FileStoreBackend` live?

Options:
- (a) `nimbus-server` — the integration point, where the HostBridge is
- (b) `nimbus-engine` — alongside `Service` and tenant management
- (c) New `nimbus-files` crate — separate concern

**Recommendation:** Start in `nimbus-server` (option a). The file store is
a host-bridge concern, not a core engine concern. If it grows complex enough
to warrant its own crate, extract later. Don't over-abstract early.

### Q4: How do file writes interact with usage metering?

Nimbus tracks monthly active users via `UsageStore`. Should file storage
usage (total bytes, operation counts) be metered similarly?

**Recommendation:** Yes, but as a later addition. Initial implementation
should track total bytes per tenant (for enforcing size limits) but full
usage metering can come after the core feature is proven.

### Q5: Should we support streaming reads/writes?

The initial API reads and writes full files. For large files (video,
datasets), streaming would be more efficient.

**Recommendation:** Defer. The initial implementation handles files up to
100MB. For larger files, the signed upload/download URL pattern offloads
the data transfer to the object storage service directly. Streaming
through the V8 runtime is complex (requires async iterator support in the
host bridge) and is better addressed as a follow-on.

### Q6: How does this interact with Convex compatibility?

Convex has `ctx.storage` with opaque IDs, not `ctx.fs` with paths. These
are different APIs serving different models.

**Recommendation:** Implement both:
- `ctx.fs` — Nimbus-native, path-based, hierarchical (Phase 1-2)
- `ctx.storage` — Convex-compatible, ID-based, flat (Phase 3)

Both can share the same `FileStoreBackend` underneath. The `ctx.storage`
compatibility shim maintains a `_storage` system table mapping IDs to
backend keys, matching Convex's model.

### Q7: What about HTTP endpoints for file access?

Users need to upload files from clients and download them. Should Nimbus
expose HTTP endpoints for direct file access?

**Recommendation:** Yes, two approaches:
- **Signed URLs** (preferred): `ctx.fs.getUrl()` and `ctx.fs.getUploadUrl()`
  generate time-limited URLs pointing directly at the object storage service.
  No data flows through Nimbus. Only works with the object storage backend.
- **Proxy endpoints** (fallback for local backend):
  `GET /api/tenants/{id}/files/{path}` and
  `POST /api/tenants/{id}/files/{path}` proxy through Nimbus. Required for
  the local filesystem backend which has no external URL.

### Q8: What consistency model for cross-function file visibility?

If function A writes a file and function B (possibly on a different node)
reads it, when is the write visible?

- **Local backend:** Immediately (same filesystem, same process).
- **Object storage backend:** After the `put()` completes. S3 has strong
  read-after-write consistency (since December 2020). GCS has always had it.
  R2 has strong consistency.

**Recommendation:** Document that read-after-write consistency is guaranteed
for the same tenant. This is true for all modern object stores and for the
local backend.

---

## References

### Nimbus Internal

- HostBridge trait: `crates/nimbus-runtime/src/host.rs:98`
- HostCallRequest: `crates/nimbus-runtime/src/host.rs:13`
- Existing ops pattern: `crates/nimbus-runtime/src/runtime.rs:575-600`
- ConvexHostBridge: `crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs`
- Tenant isolation: `crates/nimbus-engine/src/tenant.rs`
- Current JS bootstrap: `crates/nimbus-runtime/src/runtime.rs` (embedded JS in extension)

### External APIs

- Convex `ctx.storage`: <https://docs.convex.dev/file-storage>
- Cloudflare R2 Workers API: <https://developers.cloudflare.com/r2/api/workers/workers-api-reference/>
- Vercel Blob: <https://vercel.com/docs/storage/vercel-blob>
- Supabase Storage: <https://supabase.com/docs/guides/storage>
- Firebase Cloud Storage: <https://firebase.google.com/docs/storage>

### Rust Crates — Storage

- `object_store`: <https://docs.rs/object_store/latest/object_store/>
- OpenDAL: <https://github.com/apache/opendal>
- Garage (self-hosted S3): <https://garagehq.deuxfleurs.fr/>

### Rust Crates — Virtual Filesystem

- `sys_traits` (Deno team): <https://lib.rs/crates/sys_traits>
- `virtual_fs` (Wasmer): <https://docs.rs/virtual-fs/latest/virtual_fs/>
- `rust-vfs`: <https://github.com/manuel-wober/rust-vfs>

### Rust Crates — FUSE / Kernel Mount

- `fuser`: <https://github.com/cberner/fuser>
- `libfuse-fs`: <https://r2cn.dev/p/libfuse-fs>
- `ofs` (OpenDAL FUSE): <https://github.com/apache/opendal/tree/main/bin/ofs>
- ZeroFS: <https://github.com/Barre/ZeroFS>

### Filesystem-over-Object-Storage Projects

- JuiceFS: <https://github.com/juicedata/juicefs>
- Kiseki (Rust JuiceFS port, learning project): <https://github.com/crrow/kisekifs>
- RustFS (object storage server, not FS layer): <https://github.com/rustfs/rustfs>
- Curvine (cache system): <https://github.com/CurvineIO/curvine>
- s3fs-fuse: <https://github.com/s3fs-fuse/s3fs-fuse>
- goofys (unmaintained since 2020): <https://github.com/kahing/goofys>

### Research Context

- JuiceFS architecture: <https://juicefs.com/docs/community/architecture/>
- JuiceFS caching: <https://juicefs.com/docs/cloud/guide/cache/>
- S3 strong consistency (December 2020): <https://aws.amazon.com/s3/consistency/>
- GCS consistency: <https://cloud.google.com/storage/docs/consistency>
- deno_core extensions and ops: <https://github.com/nicolo-ribaudo/tc39-proposal-structs>
- deno_core ModuleLoader trait: <https://docs.rs/deno_core/latest/deno_core/trait.ModuleLoader.html>
