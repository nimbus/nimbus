# Bun/JSC Gate 20: Host Permission Lockdown Decision

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-in-process-lockdown-plan.md`

Inputs:

- `docs/plans/proof/runtime-engine/bun-jsc/gate-11-permission-surface-inventory.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-18-in-process-lockdown-source-map.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-19-resolver-package-lockdown-decision.md`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun local proof head: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

## Decision

Status: source-blocked for `in_process_untrusted`.

The current Bun/JSC non-CLI generated-wrapper lane does not have a production
permission lockdown profile. It should remain `in_process_trusted_only` because
host-sensitive surfaces are present and not routed through Nimbus policy.

The correct architecture is not to monkey-patch JavaScript globals. Nimbus
needs a Bun embedder permission profile with two enforcement classes:

- construction absence: APIs not installed in the VM/global object for the
  selected profile
- native denial or mediation: operations that remain present but synchronously
  check Nimbus policy before reaching the host

## Current Gate 11 Result

Gate 11 classified the current surface inventory:

| Surface | Current result | Required `in_process_untrusted` result |
| --- | --- | --- |
| `Bun` global | `unsafe_bypass` | absent or profile-limited |
| `Bun.file` | `unsafe_bypass` | absent or native filesystem policy hook |
| `Bun.write` | `unsafe_bypass` | absent or native filesystem policy hook |
| `Bun.spawn` | `unsafe_bypass` | absent or native subprocess policy hook |
| `Bun.spawnSync` | `unsafe_bypass` | absent or native subprocess policy hook |
| `Bun.serve` | `unsafe_bypass` | absent or native listen/server policy hook |
| `Bun.listen` | `unsafe_bypass` | absent or native listen policy hook |
| `Bun.connect` | `unsafe_bypass` | absent or native connect policy hook |
| `Bun.plugin` | `unsafe_bypass` | absent |
| `Bun.FFI` | `unsafe_bypass` | absent |
| `Bun.dlopen` | `absent_by_default` | absent |
| `Bun.FFI.dlopen` | `unsafe_bypass` | absent |
| `Bun.env` | `unsafe_bypass` | projected env or native env policy hook |
| `process` | `unsafe_bypass` | projected process object or absent |
| `process.env` | `unsafe_bypass` | projected env or native env policy hook |
| `require` | `absent_by_default` | absent unless resolver policy exists |
| Node builtins through `require` | `absent_by_default` | absent unless resolver policy exists |
| `fetch` | `unsafe_bypass` | absent or native fetch/network policy hook |
| `WebSocket` | `unsafe_bypass` | absent or native network policy hook |
| `setTimeout` | `unsafe_bypass` | allowed only with host-owned cancellation/lifecycle |
| `Worker` | `unsafe_bypass` | absent unless identity, policy, memory, and teardown propagate |
| `new Function` | `unsafe_bypass` | absent/blocked for tenant code or compile-policy hooked |
| `eval` | `unsafe_bypass` | absent/blocked for tenant code or compile-policy hooked |
| Dynamic import syntax | `policy_hook_missing` | denied or resolver-policy hooked |
| Nimbus host hooks and generated wrapper | `policy_hook_available` | allowed HostBridge path |

This table is enough to block promotion. The proof does not need to mutate Bun
to fail closed before the product decision; it already shows the current lane
is not a tenant-safe permission profile.

## Required Permission Profile

The first credible profile should be named and diagnosed as something like:

```text
RuntimeBackendKind::BunJsc
RuntimeBackendTrustTier::InProcessUntrusted
RuntimeBackendLockdownProfile::BunJscInProcessUntrusted
```

That profile can become selectable only if the following checks are true:

| Capability class | Minimum rule |
| --- | --- |
| Filesystem | Default deny. Allow only Nimbus-managed generated bundle paths, tenant-scoped named volumes, and explicit read/write grants after path canonicalization. |
| Network | Default deny. Egress/connect, DNS, UDP, TCP, TLS, fetch, WebSocket, and listen/server creation require explicit grants. |
| Environment | Default deny. Expose only a projected, immutable environment map unless a write grant exists. |
| Process | No raw host process object. `cwd`, argv, exec path, pid/uid/gid, exit, title, stdio, and system metadata must be projected or denied. |
| Subprocess | Default deny. Any future grant must check executable, args, env, cwd, stdio, IPC, inherited descriptors, timeout, and audit identity before native spawn. |
| FFI/native addons | Absent for untrusted tenants. |
| Plugins | Absent for untrusted tenants. |
| Workers | Absent until policy, identity, memory, HostBridge, cancellation, and teardown propagate into child VMs/threads. |
| Timers | Allowed only if host cancellation and VM teardown prove timers cannot outlive the invocation policy. |
| Dynamic code | Deny tenant-visible `eval`, `new Function`, Node `vm`, and equivalent compile paths unless an intrinsic-safe compile policy exists. Generated Nimbus wrappers remain host-authored code. |
| Dynamic import/resolver | Default deny unless Gate 19's resolver policy exists. |
| HostBridge | Only Nimbus-installed host calls are allowed host authority. Every host operation must carry runtime identity and capability context. |

## Source-Level Blockers

| Class | Source owners | Missing enforcement |
| --- | --- | --- |
| Bun object construction | `src/jsc/bindings/BunObject.cpp`, `src/runtime/api/BunObject.{zig,rs}` | No selected embedder profile that omits or policy-wraps sensitive `Bun.*` properties before tenant code runs. |
| Filesystem | `src/runtime/webcore/Blob.{zig,rs}`, `src/runtime/node/node_fs.{zig,rs}`, `src/runtime/node/node_fs_binding.{zig,rs}`, `src/runtime/node/types.{zig,rs}`, `src/runtime/node/path_watcher.rs` | No Nimbus filesystem grant check at BunFile/NodeFS/native path owners. |
| Network | `src/runtime/webcore/fetch.{zig,rs}`, `src/http/AsyncHTTP.*`, `src/http_jsc/websocket_client.*`, `src/jsc/bindings/webcore/WebSocket.*`, `src/runtime/socket/*`, `src/runtime/server/*`, `src/js/node/{net,tls,dgram,http,https,http2,dns}.ts` | No Nimbus egress/listen/DNS grant check before native socket/fetch/server creation. |
| Environment/process | `src/jsc/bindings/JSEnvironmentVariableMap.cpp`, `src/runtime/api/BunObject.{zig,rs}`, `src/runtime/node/node_process.{zig,rs}`, `src/jsc/bindings/BunProcess.cpp` | No projected process/env profile; host process data remains reachable. |
| Subprocess | `src/runtime/api/bun/js_bun_spawn_bindings.{zig,rs}`, `src/runtime/api/bun/subprocess.{zig,rs}`, `src/runtime/api/bun/process.{zig,rs}`, `src/spawn/*`, `src/js/node/child_process.ts` | No native spawn policy hook. |
| FFI/native addons | `src/js/bun/ffi.ts`, `src/runtime/ffi/*`, `src/runtime/napi/*` | No proof that dynamic library/native addon loading is absent or denied below JS. |
| Plugins | `src/jsc/bindings/BunPlugin.*`, `src/bundler_jsc/PluginRunner.*`, `src/jsc/bindings/ZigGlobalObject.cpp` | No profile-level plugin absence and no plugin load/resolve policy. |
| Workers | `src/jsc/web_worker.{zig,rs}`, `src/jsc/bindings/webcore/Worker.*`, `src/jsc/bindings/webcore/JSWorker.cpp`, `src/js/node/worker_threads.ts`, `BroadcastChannel` / message-port bindings | No policy, identity, HostBridge, or teardown propagation into child execution contexts. |
| Timers/lifecycle | `src/jsc/bindings/node/NodeTimers.cpp`, `src/runtime/timer/*`, `src/js/node/timers*.ts`, `src/jsc/event_loop.*`, `src/runtime/jsc_hooks.rs` | Gate 14 proves trusted reuse can recover after cancellation, not that untrusted timers cannot survive a tenant invocation. |
| Dynamic code | JSC intrinsics plus `src/jsc/bindings/bindings.cpp` `Bun__REPL__evaluate`, `src/js/node/vm.ts`, `src/jsc/bindings/NodeVM*`, `src/jsc/bindings/JSCommonJSExtensions.*` | No compile/eval policy for tenant-visible dynamic code. |

## Fork Or Upstream Shape

The permission work is too broad for a Nimbus-only wrapper and too sensitive
for a one-off local patch. The maintainable Bun-side API should be an
upstreamable embedder profile:

```text
BunEmbedderSecurityProfile {
  globals: allowlist/denylist
  bun_object_properties: allowlist/denylist
  node_globals: absent | admitted
  resolver_policy: BunEmbedderResolverPolicy
  filesystem_policy: BunEmbedderFilesystemPolicy
  network_policy: BunEmbedderNetworkPolicy
  process_policy: BunEmbedderProcessPolicy
  subprocess_policy: BunEmbedderSubprocessPolicy
  native_loading_policy: absent | admitted
  worker_policy: absent | admitted_with_child_profile
  dynamic_code_policy: generated_only | admitted
  lifecycle_policy: cancellation_and_teardown_required
}
```

If upstream is not willing to expose a profile of this shape, a Nimbus Bun fork
would be justified only after the patch surface is proven small enough to
maintain across Bun/JSC/WebKit updates.

## Verification Evidence

The blocker is based on:

- Gate 11's native inventory output, which records the present unsafe surfaces
  listed above.
- Gate 18's read-only source map, which names the owners for each surface.
- Gate 19's resolver decision, which blocks dynamic import and resolver lanes.

No Bun files were modified for this gate.

## Outcome

`BIL4` is complete as a source-blocked permission decision. Bun/JSC remains
`in_process_trusted_only`; `RuntimeBackendLockdownProfile::BunJscInProcessUntrusted`
must stay rejected until Bun exposes construction, resolver, and native
permission hooks that turn every current `unsafe_bypass` into `absent`,
`denied`, or `policy_hooked`.
