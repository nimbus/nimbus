# Bun/JSC Gate 26: Embedder API Proposal

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun proof head: `ce5aa2a389` (`Stabilize Bun embed cancellation proof on Linux`)

## Decision

Status: proposed API contract complete.

Nimbus should pursue a narrow Bun embedder API before forking Bun. The API
should make Bun/JSC embeddable as a non-CLI VM with a named security profile,
resolver policy, native permission hooks, lifecycle hooks, and audit-friendly
decision evidence. It should not know about Nimbus tenants, Convex, or
HostBridge internals; those remain embedder-provided callback context.

The minimum product-worthy shape is:

```text
BunEmbedderOptions
  -> construction profile
  -> resolver policy
  -> native permission policy
  -> dynamic-code policy
  -> worker child-profile policy
  -> lifecycle/cancellation policy
  -> audit callback
```

If Bun cannot expose this shape upstream, `BEP2` decides whether the remaining
patch surface is small enough to justify a Nimbus-maintained fork.

## Design Principles

- Deny before construction when possible; deny before native host effects when
  construction absence is not possible.
- Keep policy synchronous at sensitive entrypoints. Async policy checks cannot
  run after socket, file, subprocess, dynamic library, or worker creation.
- Treat JavaScript wrappers as defense in depth, not the isolation boundary.
- Keep host identity opaque to Bun. Bun receives an embedder context pointer
  and returns decision evidence; Nimbus owns tenant/workload identity.
- Make denial auditable: every denial should carry a stable reason code and
  optional surface metadata.
- Keep CLI behavior unchanged unless the embedder options are explicitly used.

## Proposed Construction API

Rust-like shape:

```rust
pub struct BunEmbedderOptions<'a> {
    pub profile: BunEmbedderSecurityProfile,
    pub resolver: &'a dyn BunEmbedderResolverPolicy,
    pub permissions: &'a dyn BunEmbedderPermissionPolicy,
    pub lifecycle: BunEmbedderLifecyclePolicy,
    pub audit: Option<&'a dyn BunEmbedderAuditSink>,
    pub embedder_context: *mut core::ffi::c_void,
}

pub enum BunEmbedderSecurityProfile {
    FullBunCliCompatible,
    NonCliTrustedGeneratedWrapper,
    NimbusUntrustedIsolate,
}
```

`FullBunCliCompatible` preserves current Bun defaults. `NonCliTrustedGeneratedWrapper`
matches the current proof-only lane. `NimbusUntrustedIsolate` is the profile
that must omit or deny every host-sensitive surface before Nimbus can select
Bun/JSC for untrusted tenants.

Required construction-profile fields:

| Field | `NimbusUntrustedIsolate` behavior |
| --- | --- |
| `bun_global` | absent or profile-limited to explicitly safe properties |
| `node_globals` | absent unless resolver policy allows specific builtins |
| `process_global` | projected object only; no raw host process object |
| `env_global` | projected immutable env by default |
| `worker_globals` | absent unless child-profile policy exists |
| `timer_globals` | allowed only with lifecycle-owned cancellation and teardown |
| `dynamic_code_intrinsics` | denied for tenant code; host-authored generated wrapper remains possible |
| `module_loader` | installed only with resolver policy attached before first tenant evaluation |
| `ffi_native_loading` | absent |
| `plugins` | absent |

## Resolver Policy API

```rust
pub trait BunEmbedderResolverPolicy {
    fn resolve(&self, request: BunResolveRequest<'_>) -> BunResolveDecision;
}

pub struct BunResolveRequest<'a> {
    pub specifier: &'a str,
    pub referrer: Option<&'a str>,
    pub import_kind: BunImportKind,
    pub asserted_type: Option<&'a str>,
    pub package_root_candidate: Option<&'a str>,
    pub resolved_path_candidate: Option<&'a str>,
    pub embedder_context: *mut core::ffi::c_void,
}

pub enum BunImportKind {
    StaticEsm,
    DynamicImport,
    BunResolve,
    BunResolveSync,
    ImportMetaResolve,
    CommonJsRequire,
    RequireResolve,
    NodeBuiltin,
    PluginVirtualModule,
    NativeAddon,
}

pub enum BunResolveDecision {
    Deny { reason: BunPolicyDenyReason },
    AllowGeneratedBundlePath { path: String },
    AllowBuiltin { canonical_id: String },
    AllowPackagePath { path: String, package_id: String, digest: Option<String> },
    AllowVirtualModule { namespace: String, id: String },
}
```

`NimbusUntrustedIsolate` starts deny-by-default:

- allow Nimbus-generated wrapper code and its internal helper map only
- deny dynamic import unless explicitly admitted
- deny `Bun.resolve*` and `import.meta.resolve*` unless scoped to an allowed
  generated bundle path
- deny Node builtins unless Nimbus generated an explicit Bun-compatible binding
- deny external packages, plugins, virtual modules, native addons, and dynamic
  libraries

Required source hook points:

| Surface | Hook target |
| --- | --- |
| Dynamic import/module loader | `ZigGlobalObject.cpp`, `ModuleLoader.cpp`, `JSModuleLoader.{rs,zig}` before Bun resolution succeeds |
| `Bun.resolve*` | `BunObject.{rs,zig}` `resolve`/`resolveSync` callbacks and C ABI resolver exports |
| `import.meta.resolve*` | `ImportMetaObject.cpp` before `Bun__resolveSync*` |
| CommonJS | `ExposeNodeModuleGlobals.cpp`, `CommonJS.ts`, Node module paths if `require` is installed |
| Plugins/virtual modules | `BunPlugin.*`, `PluginRunner.*`, `onLoadPlugins.resolveVirtualModule` |
| Native addons | N-API and FFI resolver/load paths before native load |

## Native Permission Policy API

```rust
pub trait BunEmbedderPermissionPolicy {
    fn filesystem(&self, request: BunFilesystemRequest<'_>) -> BunPermissionDecision;
    fn network(&self, request: BunNetworkRequest<'_>) -> BunPermissionDecision;
    fn process(&self, request: BunProcessRequest<'_>) -> BunPermissionDecision;
    fn subprocess(&self, request: BunSubprocessRequest<'_>) -> BunPermissionDecision;
    fn native_loading(&self, request: BunNativeLoadRequest<'_>) -> BunPermissionDecision;
    fn worker(&self, request: BunWorkerRequest<'_>) -> BunWorkerDecision;
    fn dynamic_code(&self, request: BunDynamicCodeRequest<'_>) -> BunPermissionDecision;
    fn timer(&self, request: BunTimerRequest<'_>) -> BunPermissionDecision;
}

pub enum BunPermissionDecision {
    Deny { reason: BunPolicyDenyReason },
    Allow,
}
```

Required request fields:

| Request | Minimum fields |
| --- | --- |
| Filesystem | operation, raw path, canonical path candidate, flags/mode, fd if any, follow-symlink behavior |
| Network | operation, host, port, protocol, resolved address candidate, bind/listen versus connect, TLS/server mode |
| Process | operation, env key, cwd, argv, pid/uid/gid/system metadata field |
| Subprocess | executable, args, env projection, cwd, stdio, IPC, inherited descriptors, timeout |
| Native loading | path, ABI kind, package/source identity, digest/evidence if known |
| Worker | script/referrer, child profile, inherited permissions, memory/cancel propagation |
| Dynamic code | source kind, referrer, host-authored versus tenant-authored, compile/eval entrypoint |
| Timer | operation, delay, repeat, keepalive, invocation/lifecycle binding |

Required source hook points:

| Class | Hook target |
| --- | --- |
| Filesystem | `Blob.{rs,zig}`, NodeFS, path watchers, file descriptor open/write/read/stat paths |
| Network | fetch, WebSocket, sockets, DNS, `Bun.connect`, `Bun.listen`, `Bun.serve`, Node net/tls/dgram/http modules |
| Env/process | `JSEnvironmentVariableMap.cpp`, `BunObject.{rs,zig}`, `node_process.{rs,zig}`, `BunProcess.cpp` |
| Subprocess | `js_bun_spawn_bindings`, subprocess/process APIs, `src/spawn/*`, Node `child_process` |
| Native loading | FFI, `dlopen`, N-API, native addon loaders |
| Plugins | construction profile should omit; if present, plugin registration/load/resolve hooks must deny by default |
| Workers | web worker and Node worker construction before thread/VM creation |
| Timers | timer creation, keepalive, event-loop scheduling, teardown cleanup |
| Dynamic code | JSC eval/function/REPL/Node VM entrypoints before tenant-authored code compiles |

## Lifecycle API

The Linux proof made lifecycle requirements explicit:

```rust
pub struct BunEmbedderLifecyclePolicy {
    pub vm_reuse: BunVmReusePolicy,
    pub memory_policy: BunMemoryPolicy,
    pub cancellation_policy: BunCancellationPolicy,
    pub worker_policy: BunWorkerLifecyclePolicy,
}

pub enum BunVmReusePolicy {
    FreshDiscard,
    RetainedTrustedOnly,
}
```

Required semantics:

- every thread that touches JSC termination or JS execution must initialize
  Bun/WebKit stack bounds before touching JSC
- owner-thread termination-exception priming must be paired with explicit
  termination-request reset before normal evaluation resumes
- product cancellation must not depend on elapsed-time sleeps; the Bun pool
  must drive cancellation through explicit lifecycle state and acknowledgement
  transitions such as `Created`, `BootstrapReady`, `GuestEntered`,
  `CancelRequested`, `Terminated`, `ResetOrDiscarded`, and `TeardownComplete`
- cancellation must be host-owned, deterministic on macOS and Linux, and
  recoverable only when the lifecycle policy says the VM can be reused
- cancellation proofs must cover before guest entry, after guest entry, sync
  CPU loops, promise/microtask work, in-flight HostBridge calls, normal
  completion, teardown, retained trusted reuse, and fresh/discard untrusted
  policy
- pending promises, timers, workers, sockets, subprocesses, and native handles
  must be absent, closed, or tied to invocation teardown
- untrusted Bun/JSC starts at `FreshDiscard` plus an outer hard memory quota
  unless Bun exposes a proven hard per-VM heap limit

## Audit Contract

```rust
pub trait BunEmbedderAuditSink {
    fn record(&self, event: BunEmbedderPolicyEvent<'_>);
}

pub struct BunEmbedderPolicyEvent<'a> {
    pub surface: BunPolicySurface,
    pub operation: &'a str,
    pub decision: BunPolicyDecisionKind,
    pub reason: Option<BunPolicyDenyReason>,
    pub stable_fields: BunPolicyStableFields<'a>,
}
```

Audit events must avoid raw tenant secrets, filesystem contents, env values,
request bodies, and full source text. Nimbus can attach tenant, workload,
decision ID, and HostBridge capability context outside Bun through
`embedder_context`.

## Proof Obligations

The API is not sufficient until the Bun proof target can demonstrate:

- dynamic `import("node:fs")` denied by resolver policy
- `Bun.resolve` and `Bun.resolveSync` denied or mediated
- `Bun.file`, `Bun.write`, NodeFS, and watchers denied without grants
- fetch/WebSocket/socket/server/DNS denied without network grants
- env/process expose only a projection
- subprocess, FFI, native addons, and plugins absent or denied
- workers denied or launched only with child profile propagation
- tenant-visible `eval`, `Function`, Node `vm`, and REPL compilation denied
- timers are either absent or bound to invocation cancellation/teardown
- cancellation and recovery pass on macOS and Linux without relying on
  elapsed-time sleeps as the product mechanism
- retained VM reuse remains trusted-only unless hard memory and cleanup
  guarantees are proven

## Outcome

`BEP1` is complete. The next gate is `BEP2`: decide the exact threshold for
staying upstream-first versus creating a Nimbus Bun fork, using this proposal
as the required API surface.
