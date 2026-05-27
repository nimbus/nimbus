# Bun/JSC Gate 18: In-Process Lockdown Source Ownership Map

Date: 2026-05-23

Nimbus plan: `docs/plans/archive/bun-jsc-in-process-lockdown-plan.md`

Nimbus worktree: `/Users/jack/src/github.com/nimbus/nimbus`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun local proof head: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

## Question

Which Bun source owners would need to participate before Nimbus could promote
the Bun/JSC proof lane from `in_process_trusted_only` to
`in_process_untrusted`?

## Scope

This is a source ownership map. It does not implement Bun lockdown and does
not claim any new containment property. Its job is to make `BIL3` through
`BIL5` concrete by naming where resolver, permission, lifecycle, and dynamic
code decisions would have to be enforced.

Running arbitrary Bun applications safely in an OCI image and microVM remains
the sandbox workload path. This document covers only the in-process runtime
engine path.

## Summary

The existing proof lane constructs a non-CLI Bun/JSC `VirtualMachine`, installs
Nimbus proof host functions, and evaluates a generated Nimbus program wrapper.
Gate 11 showed that this lane still exposes host-sensitive Bun, Web, process,
worker, dynamic-code, resolver, and module-loading surfaces.

There is no single source location that can make the lane safe. The credible
in-process architecture needs three Bun-facing policy layers:

1. VM/global construction policy: decide which globals, Bun object properties,
   Node module globals, process objects, workers, timers, and dynamic-code
   intrinsics are installed.
2. Resolver/package policy: decide which static import, dynamic import,
   `Bun.resolve*`, CommonJS, Node builtin, package root, native addon, and
   plugin resolutions are allowed.
3. Native operation policy: enforce filesystem, network, environment,
   subprocess, FFI, native addon, plugin, worker, and timer/lifecycle decisions
   at the native owner, not only through JavaScript wrappers.

JavaScript wrapper deletion can be useful defense in depth, but it is not a
complete enterprise isolation boundary because native callbacks and module
loader paths can reintroduce host authority.

## Source Ownership Map

| Surface family | Gate 11 state | Primary Bun source owners observed | Plausible enforcement shape | Nimbus wrapper only? | Fork/upstream implication |
| --- | --- | --- | --- | --- | --- |
| Bun global and property registration | `unsafe_bypass` | `src/jsc/bindings/BunObject.cpp` registers the `Bun` property table; `src/runtime/api/BunObject.zig` and `src/runtime/api/BunObject.rs` own many callbacks and lazy exports. | Add an embedder lockdown profile that can omit the whole `Bun` object or selected properties before tenant code executes. Native callbacks must also re-check policy for sensitive operations. | No. Deleting properties after global creation is brittle and does not cover native/module-loader reachability. | Prefer upstream embedder API; fork only if Bun does not accept a stable construction policy hook. |
| Filesystem APIs | `Bun.file`, `Bun.write` are `unsafe_bypass`; require-based `node:fs` is absent in this lane. | `src/runtime/webcore/Blob.zig` and `src/runtime/webcore/Blob.rs` own `Bun.file`, `Bun.write`, Blob/BunFile, and file writes. `src/runtime/node/node_fs.zig`, `src/runtime/node/node_fs.rs`, `src/runtime/node/node_fs_binding.zig`, `src/runtime/node/node_fs_binding.rs`, `src/runtime/node/types.zig`, `src/runtime/node/types.rs`, and `src/runtime/node/path_watcher.rs` own Node filesystem behavior and watchers. | Deny-by-default filesystem policy at BunFile construction, NodeFS entrypoints, path normalization, file descriptors, watches, and write/truncate/open calls. Named volumes and bundle roots must be Nimbus-managed inputs. | No. A wrapper can hide `Bun.file`, but dynamic import or future `require` can reach native filesystem owners unless native hooks exist. | Upstream hook is the right target; fork patch would be broad if it must touch every filesystem entrypoint separately. |
| Network, fetch, WebSocket, and server APIs | `fetch`, `WebSocket`, `Bun.serve`, `Bun.listen`, and `Bun.connect` are `unsafe_bypass`. | `src/jsc/bindings/BunObject.cpp` exposes Bun network properties. `src/runtime/webcore/fetch.zig`, `src/runtime/webcore/fetch.rs`, `src/http/AsyncHTTP.*`, `src/http_jsc/websocket_client.*`, `src/jsc/bindings/webcore/WebSocket.*`, `src/runtime/socket/*`, `src/runtime/server/*`, and Node modules under `src/js/node/{net,tls,dgram,http,https,http2,dns}.ts` own network clients, servers, DNS, and sockets. | Central network policy must gate outbound connect, DNS, UDP, TCP, TLS, HTTP fetch, WebSocket, and inbound listen/server creation. Egress grants should be enforced before native socket creation. | No. JS wrappers cannot be the sole boundary for sockets, server constructors, DNS, or imported Node modules. | Needs upstream embedder/network policy API or a narrow forked hook near socket/fetch/server construction. |
| Environment and process globals | `Bun.env`, `process`, and `process.env` are `unsafe_bypass`. | `src/jsc/bindings/JSEnvironmentVariableMap.cpp` owns env getter/setter behavior. `src/runtime/api/BunObject.zig` and `src/runtime/api/BunObject.rs` expose env helpers. `src/runtime/node/node_process.zig`, `src/runtime/node/node_process.rs`, and `src/jsc/bindings/BunProcess.cpp` own `process`, argv, cwd, exit, and process metadata. | Embedder profile should install a Nimbus-projected env/process object, not the host process object. Env reads/writes, argv, cwd, exit, pid/uid/gid, memory/system metadata, and `Bun.which` need explicit policy. | Partial only. Nimbus can provide a projected object, but native env/process getters must not remain reachable. | Prefer upstream global construction and env/process policy hook. |
| Subprocess, shell, IPC, and stdio inheritance | `Bun.spawn` and `Bun.spawnSync` are `unsafe_bypass`; require-based `node:child_process` is absent in this lane. | `src/jsc/bindings/BunObject.cpp` exposes `spawn` and `spawnSync`. `src/runtime/api/bun/js_bun_spawn_bindings.zig`, `src/runtime/api/bun/js_bun_spawn_bindings.rs`, `src/runtime/api/bun/subprocess.zig`, `src/runtime/api/bun/subprocess.rs`, `src/runtime/api/bun/process.zig`, `src/runtime/api/bun/process.rs`, `src/spawn/process.rs`, `src/spawn/lib.rs`, and `src/js/node/child_process.ts` own subprocess behavior. | Default deny. If ever granted, policy must check executable, args, env, cwd, stdio, IPC, timeout, and inherited descriptors before `spawn_process` / platform spawn. | No. Subprocesses must be blocked at native spawn, not only at `Bun.spawn` property access. | Requires upstream or forked native spawn policy hook. |
| FFI, `dlopen`, native addons, and N-API | `Bun.FFI` and `Bun.FFI.dlopen` are `unsafe_bypass`; `Bun.dlopen` is absent; require-based native addons are absent in this lane. | `src/js/bun/ffi.ts`, `src/runtime/ffi/*`, and `src/runtime/napi/*` own FFI, dynamic library loading, and Node native addon/N-API paths. Resolver hooks around embedded node files are in `src/runtime/jsc_hooks.rs`. | Keep absent for untrusted tenants. If ever granted, require signed native artifact policy, path policy, ABI policy, and per-tenant load isolation. | No. Native code loading is a hard boundary and must be absent or native-policy gated. | A selectable in-process untrusted backend should not ship until upstream/fork hooks can prove FFI/native addon absence. |
| Plugins and bundler loader hooks | `Bun.plugin` is `unsafe_bypass`. | `src/jsc/bindings/BunObject.cpp` exposes `plugin`. `src/jsc/bindings/BunPlugin.cpp`, `src/jsc/bindings/BunPlugin.h`, `src/bundler_jsc/PluginRunner.zig`, `src/bundler_jsc/PluginRunner.rs`, and `src/jsc/bindings/ZigGlobalObject.cpp` own plugin registration and module-loader integration. | Omit plugin APIs in the in-process untrusted profile. If later allowed, plugin registration, `onResolve`, `onLoad`, virtual modules, and plugin-generated code need policy and artifact provenance. | No. Plugins affect module loading and generated code paths below user-visible JS. | Prefer upstream construction/profile hook; fork if plugin system cannot be disabled in embed mode. |
| Resolver, dynamic import, package roots, and Node builtins | Dynamic `import("node:fs")` was `policy_hook_missing`/fulfilled in Gate 13; `Bun.resolve` and `Bun.resolveSync` are `unsafe_bypass`; `require` is absent by default. | `src/runtime/api/BunObject.zig` and `src/runtime/api/BunObject.rs` own `Bun.resolve*`. `src/runtime/jsc_hooks.rs` contains resolver hook plumbing. `src/jsc/bindings/ImportMetaObject.cpp` owns `import.meta.resolve`, `resolveSync`, and require setup. `src/js/builtins/CommonJS.ts` owns CommonJS resolution helpers. `src/jsc/bindings/ZigGlobalObject.cpp`, `src/jsc/bindings/ModuleLoader.cpp`, `src/jsc/JSModuleLoader.*`, and `src/resolver/*` own module loading and package resolution. | Nimbus needs a Bun-owned resolver policy distinct from Deno/V8 `node_external_packages`: generated bundle root allowlist, Node builtin deny/allow map, dynamic import policy, native addon/plugin denial, package root constraints, and audit evidence. | Partial. Generated Nimbus helper maps can deny their own imports, but they do not control Bun's dynamic import/module loader. | Upstream resolver hook is the cleanest seam; fork patch likely if no stable embedder resolver policy exists. |
| CommonJS `require` and Node module globals | `require` and require-based Node builtins are absent by default in the current non-CLI generated-wrapper lane. | `src/jsc/bindings/ExposeNodeModuleGlobals.cpp` owns builtin alias exposure and `Bun__REPL__setupGlobalRequire`. `src/jsc/modules/NodeModuleModule.cpp` and `src/jsc/bindings/JSCommonJSExtensions.*` own Node module internals. | Keep absent for the first in-process profile. If enabled later, every Node builtin and extension must be admitted through the Bun resolver policy. | Yes only while it remains absent at construction. Once installed, native/module hooks are required. | No fork required if Nimbus never calls setup. Upstream/fork required if product needs selective Node builtins in Bun/JSC. |
| Workers, worker threads, `BroadcastChannel`, and message ports | `Worker` is `unsafe_bypass`; require-based `node:worker_threads` is absent by default. | `src/jsc/web_worker.zig`, `src/jsc/web_worker.rs`, `src/jsc/bindings/webcore/Worker.*`, `src/jsc/bindings/webcore/JSWorker.cpp`, `src/js/node/worker_threads.ts`, `src/jsc/bindings/webcore/BroadcastChannel.*`, and message-port bindings own worker creation and cross-context messaging. | Default deny for untrusted in-process Bun/JSC. Any worker grant must propagate tenant/runtime identity, HostBridge policy, timers, memory limits, and cancellation into the child VM/thread. | No. Worker constructors create new execution contexts below a wrapper boundary. | Needs upstream/fork construction hook plus lifecycle propagation. |
| Timers, sleep, microtasks, and event-loop progress | `setTimeout` is `unsafe_bypass`; `Bun.sleep`/`sleepSync` are part of the Bun object. | `src/jsc/bindings/node/NodeTimers.cpp`, `src/jsc/bindings/headers.h` timer exports, `src/runtime/timer/*`, `src/js/node/timers.ts`, `src/js/node/timers.promises.ts`, `src/jsc/event_loop.*`, and `src/runtime/jsc_hooks.rs` own timers and event-loop hooks. `src/jsc/bindings/BunObject.cpp` exposes `sleep` and `sleepSync`. | Timers can exist only if invocation cancellation, keepalive handling, timer heap cleanup, and event-loop drain are host-owned. Long-lived timers must not outlive the invocation unless the lifecycle policy explicitly allows retained trusted VMs. | No for lifecycle safety. Hiding timer globals is not enough if imported modules or native APIs can enqueue work. | Upstream hook may be needed for robust cancellation/teardown; fork only after BIL5 identifies an exact gap. |
| Dynamic code: `eval`, `Function`, Node `vm`, and REPL evaluation | `eval` and `new Function` are `unsafe_bypass`; Node `vm` is absent unless the relevant module is loaded. | JSC language intrinsics provide `eval` and `Function`. Bun's proof evaluator uses `src/jsc/bindings/bindings.cpp` `Bun__REPL__evaluate`. Node VM ownership lives under `src/js/node/vm.ts` and `src/jsc/bindings/NodeVM.*`, `NodeVMScript.*`, `NodeVMModule.*`, and `JSCommonJSExtensions.*`. | Generated Nimbus wrappers may use controlled code generation, but tenant-visible dynamic code needs a real compile/eval policy or an immutable lockdown profile that removes/blocks the intrinsics before tenant code runs. Node `vm` should stay absent. | No. Deleting `globalThis.eval` and `Function` after bootstrap is not a complete intrinsic lockdown story by itself. | Likely requires upstream/JSC-aware embedder policy, or the backend remains trusted-only. |
| Nimbus host calls and generated wrapper | `policy_hook_available` | Bun proof code under `src/embed_probe/*` installs `__nimbusHostCall`, `__nimbusAsyncHostCall`, `__nimbusCreateContext`, and the generated wrapper fixture. Nimbus runtime-side analogs live behind the runtime engine seam, not in Bun product code yet. | Keep this as the only allowed host authority path. Every future Bun/JSC host operation must be either absent or mediated through an equivalent Nimbus HostBridge/capability decision. | Yes for Nimbus-owned generated helper code, not for Bun-owned host surfaces. | No fork implied by host-call proof alone; fork/upstream decision depends on the unsafe Bun-owned surfaces above. |

## Architecture Implication

The first selectable Bun/JSC backend cannot be "Bun with some globals hidden."
It must be a named lockdown profile whose cache key, diagnostics, and admission
decision identify:

- `RuntimeBackendKind::BunJsc`
- a trust tier no stronger than the proven hooks support
- a lockdown profile describing installed globals and native policy hooks
- the resolver/package policy in force
- lifecycle policy for VM reuse, cancellation, workers, timers, and memory

`BIL1` added the Nimbus-side typed axes. This map shows that the Bun-side
implementation needs either upstream embedder APIs or a small maintained fork
patch; Nimbus wrappers alone are insufficient for `in_process_untrusted`.

## Recommended Next Proof Work

`BIL3` should focus on resolver/package lockdown first because Gate 13 already
proved the biggest sharp edge: dynamic `import("node:fs")` can be fulfilled
even though `require` is absent in the non-CLI generated-wrapper lane.

`BIL4` should then split permission proof into two categories:

- construction absence: globals/APIs that can be omitted before tenant code
  executes
- native denial: operations that remain present but synchronously fail through
  a policy hook before reaching the host

`BIL5` should not assume retained VM reuse for untrusted tenants unless Bun/JSC
exposes a hard memory boundary and teardown can prove no timers, workers,
subprocess handles, sockets, promises, or native resources survive the
invocation.

## Verification

Source searches were run read-only against `/Users/jack/src/github.com/oven-sh/bun`
with `rg` for these API families:

- `Bun.file`, `Bun.write`, NodeFS, and file watchers
- `Bun.spawn`, `Bun.spawnSync`, `child_process`, and subprocess internals
- `Bun.serve`, `Bun.listen`, `Bun.connect`, sockets, fetch, WebSocket, DNS,
  and Node network modules
- `Bun.resolve`, `Bun.resolveSync`, `import.meta.resolve`, CommonJS, dynamic
  import, module loader, and resolver internals
- `dlopen`, FFI, native addons, and N-API
- `Worker`, `worker_threads`, `BroadcastChannel`, and message ports
- process and environment getters/setters
- timers, sleep, `setTimeout`, event loop, and dynamic-code/Node VM owners

No Bun source files were modified for this gate.
