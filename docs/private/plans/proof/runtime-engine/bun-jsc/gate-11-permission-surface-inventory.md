# Bun/JSC Gate 11: Permission Surface Inventory

Date: 2026-05-23

Nimbus prior proof revision: `ba75d599` (`Refresh Bun JSC proof plan`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `c57f7e58c0` (`Add Bun embed timeout cancel proof`)

Bun proof commit: `9e20ac28a2` (`Add Bun embed permission inventory proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

When the non-CLI Bun/JSC embed target loads the same generated Nimbus program
wrapper used by Gate 10, which host-sensitive JavaScript surfaces are present,
absent, denied, policy-hooked, or unhooked?

## Scope

This gate is an inventory gate. It does not claim containment for present Bun,
Node, Web, worker, dynamic-code, filesystem, network, process, or environment
surfaces. The only accepted production implication is negative: any present
host-sensitive surface without a concrete Nimbus policy hook keeps Bun/JSC in
`in_process_trusted_only`.

## Patch Shape

The Bun proof commit adds a sixth C ABI probe to the non-CLI native smoke
target:

```text
nimbus_bun_embed_probe_permission_surface_inventory()
```

`scripts/build/bun.ts` wires that probe into `check-bun-embed-probe` after the
construct/destroy, sync host-call, async host-call, generated-program, and
timeout/cancel probes.

`src/embed_probe/lib.rs` now:

1. creates a fresh non-CLI `VirtualMachine` with `InitOptions::is_main_thread = false`,
2. installs the proof `__nimbusHostCall` and `__nimbusAsyncHostCall` JSC host
   functions,
3. installs a minimal `__nimbusCreateContext`,
4. loads the generated Nimbus program wrapper,
5. evaluates a fixed inventory table through `Bun__REPL__evaluate`, and
6. prints one classification per surface.

Classification values:

| Value | Meaning |
| --- | --- |
| `absent_by_default` | The surface is not present in this non-CLI generated-wrapper VM. |
| `denied_by_default` | The surface is present but operation attempts are denied before host authority. This gate observed no rows in this class. |
| `policy_hook_available` | The surface is present and already routes through a Nimbus-owned hook in the proof. |
| `policy_hook_missing` | The surface can be requested, but there is no Nimbus resolver or policy hook yet. |
| `unsafe_bypass` | The surface is present without a Nimbus policy hook and must be treated as host authority. |

## Probe Source Snippets

The inventory uses these JavaScript snippets in the Bun proof commit. Where a
snippet contains `<property>`, the committed probe instantiates the exact same
expression once for every property listed in the preceding comment.

```js
// Bun global
typeof globalThis.Bun === "undefined" ? 1 : 5
```

```js
// Bun.file, Bun.write, Bun.spawn, Bun.spawnSync, Bun.serve, Bun.listen,
// Bun.connect, Bun.plugin, Bun.FFI, Bun.env
typeof globalThis.Bun === "undefined"
  ? 1
  : (typeof globalThis.Bun.<property> === "undefined" ? 1 : 5)
```

```js
// Bun.dlopen
typeof globalThis.Bun === "undefined"
  ? 1
  : (typeof globalThis.Bun.dlopen === "undefined" ? 1 : 5)
```

```js
// Bun.FFI.dlopen
typeof globalThis.Bun === "undefined"
  ? 1
  : (typeof globalThis.Bun.FFI === "undefined"
    ? 1
    : (typeof globalThis.Bun.FFI.dlopen === "undefined" ? 1 : 5))
```

```js
// process
typeof globalThis.process === "undefined" ? 1 : 5
```

```js
// process.env
typeof globalThis.process === "undefined"
  ? 1
  : (typeof globalThis.process.env === "undefined" ? 1 : 5)
```

```js
// require, Node builtin modules through require, node:fs, fs,
// node:child_process, node:worker_threads, node:net, node:dgram, node:ffi,
// and native add-ons through require
typeof globalThis.require === "undefined" ? 1 : 5
```

```js
// fetch, WebSocket, setTimeout, Worker
typeof globalThis.<property> === "undefined" ? 1 : 5
```

```js
// new Function
typeof globalThis.Function === "function" ? 5 : 1
```

```js
// eval
typeof globalThis.eval === "function" ? 5 : 1
```

```js
// dynamic import syntax
(() => {
  try {
    new Function("return import('node:fs')");
    return 4;
  } catch (_) {
    return 1;
  }
})()
```

```js
// Nimbus host hooks and generated wrapper
typeof globalThis.__nimbusHostCall === "function"
  && typeof globalThis.__nimbusAsyncHostCall === "function"
  && typeof globalThis.__nimbusInvoke === "function"
    ? 3
    : 4
```

## Inventory Result

Final native proof output:

| Surface | Result |
| --- | --- |
| `Bun` global | `unsafe_bypass` |
| `Bun.file` | `unsafe_bypass` |
| `Bun.write` | `unsafe_bypass` |
| `Bun.spawn` | `unsafe_bypass` |
| `Bun.spawnSync` | `unsafe_bypass` |
| `Bun.serve` | `unsafe_bypass` |
| `Bun.listen` | `unsafe_bypass` |
| `Bun.connect` | `unsafe_bypass` |
| `Bun.plugin` | `unsafe_bypass` |
| `Bun.FFI` | `unsafe_bypass` |
| `Bun.dlopen` | `absent_by_default` |
| `Bun.FFI.dlopen` | `unsafe_bypass` |
| `Bun.env` | `unsafe_bypass` |
| `process` | `unsafe_bypass` |
| `process.env` | `unsafe_bypass` |
| `require` | `absent_by_default` |
| Node builtin modules through `require` | `absent_by_default` |
| `node:fs` through `require` | `absent_by_default` |
| `fs` through `require` | `absent_by_default` |
| `node:child_process` through `require` | `absent_by_default` |
| `node:worker_threads` through `require` | `absent_by_default` |
| `node:net` through `require` | `absent_by_default` |
| `node:dgram` through `require` | `absent_by_default` |
| `node:ffi` through `require` | `absent_by_default` |
| Native add-ons through `require` | `absent_by_default` |
| `fetch` | `unsafe_bypass` |
| `WebSocket` | `unsafe_bypass` |
| `setTimeout` | `unsafe_bypass` |
| `Worker` | `unsafe_bypass` |
| `new Function` | `unsafe_bypass` |
| `eval` | `unsafe_bypass` |
| Dynamic `import(...)` syntax | `policy_hook_missing` |
| Nimbus host hooks and generated wrapper | `policy_hook_available` |

## Source Ownership

| Surface family | Bun source owner observed | Nimbus implication |
| --- | --- | --- |
| Bun object and Bun filesystem/process/network APIs | `src/jsc/bindings/BunObject.cpp` `bunObjectTable`, plus `src/runtime/api/BunObject.zig` callback/lazy-property exports. `file`, `write`, `spawn`, `serve`, `listen`, `connect`, `plugin`, `FFI`, and `env` are registered there. | A product Bun backend needs explicit allow/deny hooks or must avoid exposing the Bun object to untrusted code. |
| FFI and native loading | `src/runtime/api/BunObject.zig` exposes `Bun.FFI`; `src/runtime/ffi/FFIObject.rs` owns FFI operations including `dlopen`. | Native library loading must stay denied or be admitted through a first-class native capability. |
| Process and environment | `src/runtime/node/node_process.rs` owns `globalThis.process` / `node:process`; `src/jsc/bindings/ZigGlobalObject.cpp` owns the process and env lazy objects. | Environment reads and process state are host authority until policy hooks exist. |
| CommonJS require and Node builtin aliases | `src/jsc/bindings/ExposeNodeModuleGlobals.cpp` owns builtin alias exposure and `Bun__REPL__setupGlobalRequire`. | The non-CLI proof does not call the REPL setup path, so `require` and require-based Node modules are absent in this lane. If enabled later, every builtin must be separately admitted. |
| Web/network APIs | `src/runtime/webcore/fetch.rs`, `src/jsc/bindings/BunObject.cpp` `fetch`, `src/jsc/bindings/webcore/JSWebSocket.cpp`, and socket/server runtime code own fetch/WebSocket/network operations. | Network egress must be denied or routed through a sandbox/network policy hook before Bun/JSC can run tenant code. |
| Timers and workers | `src/runtime/timer/Timer.*`, `src/jsc/bindings/webcore/Worker.*`, and `src/jsc/web_worker.*` own timer and worker behavior. | Lifecycle/cancellation must cover timers and worker-spawned execution, not only top-level invocation promises. |
| Dynamic code and dynamic import | JSC language globals expose `eval`/`Function`; Bun module loading flows through `src/jsc/VirtualMachine.*`, `src/jsc/ModuleLoader.*`, and JS module loader bindings. | Generated-wrapper `new Function` is acceptable only for Nimbus-owned generated code. User package/module loading needs an explicit Bun resolver policy. |
| Nimbus host calls | The proof installs `__nimbusHostCall`, `__nimbusAsyncHostCall`, and generated `__nimbusInvoke` in `src/embed_probe/lib.rs`. | This is the only observed `policy_hook_available` row. A production backend would need equivalent hooks implemented behind the Nimbus runtime engine seam. |

## Verification

Formatting:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
cargo fmt --all --check
```

Result: passed.

Native proof target:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
```

Result:

```text
[configured] bun-debug in 521ms (unchanged)
ninja: Entering directory `/private/tmp/nimbus-bun-embed-native'
[0/4] cargo bun_embed_probe -> libbun_embed_probe.a (--target aarch64-apple-darwin)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 25.09s
[1/4] cxx obj/src/jsc/bindings/BunProcess.cpp.o
[2/4] link bun-embed-probe
[3/4] bun-embed-probe
nimbus bun embed permission surface inventory:
  Bun global: unsafe_bypass
  Bun.file: unsafe_bypass
  Bun.write: unsafe_bypass
  Bun.spawn: unsafe_bypass
  Bun.spawnSync: unsafe_bypass
  Bun.serve: unsafe_bypass
  Bun.listen: unsafe_bypass
  Bun.connect: unsafe_bypass
  Bun.plugin: unsafe_bypass
  Bun.FFI: unsafe_bypass
  Bun.dlopen: absent_by_default
  Bun.FFI.dlopen: unsafe_bypass
  Bun.env: unsafe_bypass
  process: unsafe_bypass
  process.env: unsafe_bypass
  require: absent_by_default
  Node builtin modules via require: absent_by_default
  node:fs via require: absent_by_default
  fs via require: absent_by_default
  node:child_process via require: absent_by_default
  node:worker_threads via require: absent_by_default
  node:net via require: absent_by_default
  node:dgram via require: absent_by_default
  node:ffi via require: absent_by_default
  native addon via require: absent_by_default
  fetch: unsafe_bypass
  WebSocket: unsafe_bypass
  setTimeout: unsafe_bypass
  Worker: unsafe_bypass
  new Function: unsafe_bypass
  eval: unsafe_bypass
  dynamic import syntax: policy_hook_missing
  Nimbus host hooks and generated wrapper: policy_hook_available
[build] check-bun-embed-probe done
```

Observed upstream warnings remained unchanged from prior proof runs:

- `bun_crash_handler`: 3 unnecessary `unsafe` warnings
- `bun_spawn`: 1 unused-label warning
- `bun_install`: 1 unused-label warning
- `bun_runtime`: 2 unnecessary `unsafe` warnings

Whitespace check:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
git diff --check
```

Result: passed.

## Decision

Status: permission inventory completed, containment not proven.

The result is stronger than a generic "needs more work" statement. The current
non-CLI Bun/JSC VM exposes substantial host authority by default: Bun file,
process, subprocess, server/listener/socket, plugin, FFI, env, fetch,
WebSocket, timers, workers, `eval`, and `new Function` surfaces are all
available without Nimbus policy hooks. `require` and require-based Node module
loading are absent only because this proof path does not call the REPL global
require setup. Dynamic `import(...)` syntax can be constructed, but there is no
Nimbus-owned Bun resolver policy.

Bun/JSC therefore remains `in_process_trusted_only` and proof-only. The next
gate should measure memory behavior under generated Nimbus invocation load
without adding any production Bun selector, runtime route, or codegen target.
