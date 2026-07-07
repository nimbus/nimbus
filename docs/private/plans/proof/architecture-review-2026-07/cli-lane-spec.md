# CLI Lane Spec — GR5 (path-sanitization truth-up), DE6 (unix-socket HTTP), DE7 (stub disposition), DE8 (adapter resolver)

Design authority: `architecture-review-2026-07-plan.md` rows + the
2026-07-07 cli-lane inventory. Crate scope: `nimbus-cli` +
`nimbus-core` (GR5 helper) + `nimbus-runtime`/`nimbus-convex`/
`nimbus-node` (GR5 call sites). Pre-launch: breaking changes preferred.

## GR5 — path-sanitization consolidation (SCOPE CORRECTED)

The plan's "13 independent implementations, route through FsCaps/cap-std"
is wrong on inventory. The sites split into domains that share NO
substrate, and most are ALREADY consolidated. Do NOT force them together.

### Facts

- FS-virtual traversal is already a choke point:
  `nimbus-fs/src/resolver.rs:86 normalize_virtual_path` (fs_grants.rs no
  longer exists — logic moved to resolver.rs + caps.rs). Backends
  (memfs, object/mod) receive already-validated `CheckedPath` with NO
  independent traversal logic. Grant enforcement is `caps.rs:111`.
- URL-authority is already consolidated:
  `nimbus-egress/src/policy.rs:767 canonicalize_authority_host` is the
  single source; `nimbus-proxy/src/request.rs` DELEGATES to it
  (canonicalize_proxy_host :302) with a parser-differential cross-check.
- URL-path-segment is already shared:
  `nimbus-machine/src/api.rs:106 machine_api_path_segment` (percent-
  encodes ids into one segment) is used by both the DE6 host client and
  the guest server.
- `nimbus-runtime/src/runtime_capabilities/paths.rs` operates on REAL
  host paths under RuntimeBundle roots, driven by Deno FsPermissions
  (starts_with(root) + symlink canonicalize) — a DIFFERENT domain from
  NimbusFS virtual paths; cap-std cannot serve it.
- The only genuine duplication is PURE-LEXICAL, zero-I/O string checks:
  `paths.rs:458 normalize_absolute_path_lexically` (near-duplicate of
  resolver.rs's ParentDir fold), `nimbus-convex/src/registry/loading.rs:538
  validate_relative_manifest_path`, and the `/../` reject in
  `nimbus-node/src/host_lifecycle.rs:868 trusted_runner_bundle_path`.
  nimbus-core has no such helper today (resource_path.rs is Firestore
  paths, not fs traversal).

### Target (normative)

1. Add ONE pure-lexical helper module to nimbus-core (zero I/O —
   respects the crate invariant): `path_lexical.rs` with
   `normalize_absolute_lexical(path) -> Result<PathBuf, LexicalPathError>`
   (RootDir-clears / CurDir-skips / ParentDir-pops-with-escape-reject /
   Prefix-reject) and `reject_relative_traversal(path) ->
   Result<(), LexicalPathError>` (empty/absolute/Prefix/RootDir/ParentDir
   reject). Behavior must be the union of the three existing lexical
   checks — verify each caller's current semantics are preserved
   exactly (write the helper's tests to each caller's current rejection
   set FIRST, then swap).
2. Route the three pure-lexical sites through it:
   `runtime paths.rs:458` (the lexical fold only — keep its symlink/
   canonicalize/starts_with(root) logic in place), `convex loading.rs:538`,
   `node host_lifecycle.rs:868` (the `/../` string reject only).
3. Do NOT touch the fs-virtual choke point (resolver.rs), the
   URL-authority canonicalizer (egress policy.rs), the URL-segment
   encoder (machine api.rs), or the CLI walk-boundary
   (path_boundary.rs) — record in the ledger that they are correctly
   domain-separated and already single-source. This is a truth-up plus
   one small helper, NOT a 13-way merge.

## DE6 — replace the hand-rolled unix-socket HTTP/1 client

### Facts
`nimbus-cli/src/machine/client.rs:321-539`: `read_unix_http_request`,
`parse_http_json_body`, `http_response_body_offset` (dup of
proxy request.rs:273 `find_header_end`), `expected_http_response_len`,
`parse_http_status_code`, `machine_api_status_error`. All sync over
`std::os::unix::net::UnixStream`. `local_server_client.rs` is reqwest-
over-TCP and does NOT do UDS — not reusable. nimbus-cli deps: reqwest +
tokio only, no hyper/hyperlocal.

### Target
1. Add `hyper` + `hyper-util` + `http-body-util` + `hyperlocal` (or
   hyper's `UnixStream` connector) as nimbus-cli deps (check workspace
   for existing versions — hyper is almost certainly already a
   transitive workspace dep via pingora/reqwest; pin to the workspace
   version). Replace the ~215 hand-rolled lines with a UDS HTTP/1.1
   client call.
2. SEQUENCING GATE: `windows-machine-support-plan.md` WIN4 adds a
   Windows named-pipe transport to THIS client. Structure the rewrite
   so the transport (UDS stream) is behind a small trait/connector the
   Windows path can substitute — do NOT hardcode UnixStream at the
   request layer. Coordinate: read WIN4 before finalizing the seam
   shape; leave a documented extension point.
3. If going async pulls call sites async, that is acceptable
   (pre-launch) — thread it through. If the churn is large, STOP and
   re-scope in the ledger rather than half-migrating.
4. Preserve every existing behavior: the traversal-shaped-id tests
   (client.rs:1337,1349), Content-Length conflict rejection, the
   deliberate `Connection: close` ignore for one-shot servers, the
   400/422→InvalidInput / 401/403→PermissionDenied mapping.

## DE7 — stub disposition (KEEP, minimal cleanup)

### Facts
`nimbus-cli/src/machine/stub/*` are the `#[cfg(not(unix))]` Windows
counterparts (paired in machine/mod.rs:10-39), LIVE Windows build
surface, not dead code. `windows-machine-support-plan.md` WIN2 REPLACES
them with `#[cfg(target_os="windows")]` real modules; plan 133-135
directs DE7 to "wire, not delete" any stub WIN2 consumes.

### Target
1. Do NOT delete the stubs. Record in the ledger that they are live
   non-unix build surface owned by WIN2.
2. The ONLY cleanup: audit the `#[allow(dead_code)]` / `allow(unused_imports)`
   cluster (stub/api.rs:1,29,40; stub/backend.rs:10; stub/client.rs:12,17;
   stub/manager.rs:15). For each, either (a) remove the allow if the item
   is actually used on the non-unix path, or (b) keep it with a one-line
   comment `// realized by WIN2` if WIN2 will fill the body. Verify with
   a cross-compile check if available; otherwise reason from mod.rs
   wiring and note it.
3. This is a DE7 = mostly no-action-with-reason item. Keep it small.

## DE8 — adapter resolver decomposition

### Facts
`nimbus-cli/src/start/adapters.rs:237-257` dispatcher builds one lazy
`CredentialStore` and calls six resolvers → `AdapterEnablement`
(:76-83, six Option<Config>). Each resolver (convex_tenancy :270,
firebase :302, cloudflare :324 — the only app_dir user, mongodb :350,
dynamodb :504, s3 :574) is a self-contained env-gate + credential-mode
block. Shared: ensure_host_opt_in, adapter_bind_addr, host_is_loopback_name,
port consts.

### Target
1. New `start/adapters/` dir: one module per adapter
   (`convex_tenancy.rs`, `firebase.rs`, `cloudflare.rs`, `mongodb.rs`,
   `dynamodb.rs`, `s3.rs`), each exposing
   `resolve(ctx: &AdapterResolveCtx) -> Result<Option<Config>, BootError>`
   where `AdapterResolveCtx` carries `{ command, env, app_dir,
   credential_store (lazy), allow_network, ... }` — the shared inputs
   the six resolvers read today.
2. `start/adapters/mod.rs` (thin composition root): `AdapterEnablement`
   struct, the dispatcher iterating the six `resolve` calls,
   `status_lines`/`apply_to`. Shared helpers (ensure_host_opt_in,
   adapter_bind_addr, host_is_loopback_name, port consts,
   DEFAULT_WIRE_TENANT) move to `start/adapters/shared.rs` or stay at
   mod root if truly shared.
3. Behavior-preserving: identical env gating, conflict guards
   (--no-mongodb/--no-dynamodb/--no-s3 with port/user), port fallbacks
   (skip-with-warn), credential modes (bound/unbound/store-signed). The
   existing adapters tests pass unmodified except imports.

## Hard constraints

- nimbus-core stays zero-I/O (GR5 helper is string-only).
- GR5 must not change any rejection behavior at any call site — it is
  a consolidation, proven by tests written to current semantics.
- DE6's transport seam must accommodate WIN4's named pipe.
- DE7 must not break the non-unix build (it is the reason the stubs
  exist).

## Verification gates (worktree root, report real counts)

```
cargo fmt --all --check
cargo clippy -p nimbus-core -p nimbus-cli -p nimbus-runtime -p nimbus-convex -p nimbus-node --all-targets -- -D warnings
cargo test -p nimbus-core -p nimbus-cli -p nimbus-runtime -p nimbus-convex -p nimbus-node
cargo check -p nimbus-server
```

If a cross-compile oracle for the non-unix stub path is available
(`cargo check --target x86_64-pc-windows-msvc` or the CI windows-check
lane), run it for DE6/DE7; otherwise say so. Update ledger rows
GR5/DE6/DE7/DE8 with evidence (GR5 and DE7 largely as truth-ups).
