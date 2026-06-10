# Bun/JSC Gate 13: Package And Module Policy

Date: 2026-05-23

Nimbus prior proof revision: `02fd50e0` (`Record Bun memory behavior proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `f6c87be47e` (`Add Bun embed memory behavior proof`)

Bun proof commit: `f0cee692c0` (`Add Bun embed package module policy proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Which package/module artifact shape is safe for the next Bun/JSC proof lane,
and what resolver authority remains unmediated in the non-CLI generated-wrapper
VM?

## Scope

This gate measures artifact and resolver behavior. It does not add a production
Bun selector, runtime route, codegen target, or package resolver. It keeps
Bun/JSC proof-only and `in_process_trusted_only`.

## Patch Shape

The Bun proof commit adds an eighth C ABI probe to the non-CLI native smoke
target:

```text
nimbus_bun_embed_probe_package_module_policy()
```

`scripts/build/bun.ts` wires that probe into `check-bun-embed-probe` after the
memory behavior probe.

`src/embed_probe/lib.rs` now:

1. creates a fresh non-CLI `VirtualMachine`,
2. installs a minimal generated-wrapper-compatible `__nimbusCreateContext`,
3. loads the generated Nimbus program wrapper through `Bun__REPL__evaluate`,
4. proves `globalThis.__nimbusInvoke` is available,
5. verifies static ESM syntax is rejected by the program evaluator,
6. executes dynamic `import("node:fs")` and records whether Bun fulfills it,
7. verifies `require` remains absent in this lane,
8. records `Bun.resolve` and `Bun.resolveSync` exposure, and
9. verifies generated Node builtin and external-package helper maps deny
   missing entries by default.

## Probe Source Snippets

Program wrapper load:

```js
typeof globalThis.__nimbusInvoke === "function" ? 1 : 0
```

Static ESM in the program evaluator:

```js
import { readFile } from "node:fs"; 1
```

Dynamic Node builtin import:

```js
import("node:fs").then(() => 7, () => 6)
```

CommonJS require:

```js
typeof globalThis.require === "undefined" ? 1 : 5
```

Bun resolver APIs:

```js
typeof globalThis.Bun === "undefined"
  ? 1
  : (typeof globalThis.Bun.resolve === "undefined" ? 1 : 5)
```

```js
typeof globalThis.Bun === "undefined"
  ? 1
  : (typeof globalThis.Bun.resolveSync === "undefined" ? 1 : 5)
```

Generated Node builtin missing-entry check:

```js
(() => {
  globalThis.__nimbusNodeBuiltinModules = new Map();
  try {
    nodeBuiltinModule("node:fs");
    return 5;
  } catch (error) {
    return String(error && error.message || error).includes("missing generated Node.js builtin binding")
      ? 2
      : 4;
  }
})()
```

Generated external-package missing-entry check:

```js
(() => {
  globalThis.__nimbusNodeExternalPackages = new Map();
  try {
    nodeExternalPackage("left-pad");
    return 5;
  } catch (error) {
    return String(error && error.message || error).includes("missing generated Node.js external package binding")
      ? 2
      : 4;
  }
})()
```

## Result

Final native proof output:

| Surface | Result |
| --- | --- |
| artifact shape | `self_contained_program_wrapper` |
| evaluation format | `program_via_Bun__REPL__evaluate` |
| static ESM import in program evaluator | `rejected` |
| dynamic `import("node:fs")` | `unsafe_import_fulfilled` |
| `require` | `absent_by_default` |
| `Bun.resolve` | `unsafe_bypass` |
| `Bun.resolveSync` | `unsafe_bypass` |
| generated Node builtin empty map | `denied_by_generated_wrapper` |
| generated external-package empty map | `denied_by_generated_wrapper` |
| selected next lane | `program_wrapper` |
| required resolver API | `nimbus_owned_bun_package_resolver` |

## Source Ownership

| Surface | Bun or Nimbus source owner observed | Nimbus implication |
| --- | --- | --- |
| Program evaluator | `src/jsc/bindings/bindings.cpp` `Bun__REPL__evaluate` builds `SourceProviderSourceType::Program` and calls `JSC::evaluate`. | The near-term Bun proof artifact should stay a self-contained program wrapper. |
| ESM evaluator | `src/jsc/JSModuleLoader.rs` wraps `JSC__JSModuleLoader__evaluate`; prior proof history recorded this path as fragile in the bare embedder. | Do not switch the next lane to Bun ESM until a safe module-loader contract is proven. |
| Dynamic import | `src/jsc/bindings/bindings.cpp` exposes module import helpers and `src/jsc/bindings/ModuleLoader.cpp` owns module fetch/evaluation. | Dynamic import is active enough to fulfill `node:fs`; tenant code needs a Nimbus-owned import hook or this backend remains trusted/sandbox-only. |
| CommonJS require | `src/jsc/bindings/ExposeNodeModuleGlobals.cpp` installs `require` only through `Bun__REPL__setupGlobalRequire`; the proof does not call it. | `require` is absent by default in this lane, but enabling it later must be a separate admitted package-loading decision. |
| Bun resolver APIs | `src/runtime/api/BunObject.rs` owns `Bun.resolve`, `Bun.resolveSync`, and resolver calls into `VirtualMachine::resolve_maybe_needs_trailing_slash`. | These resolver APIs are host filesystem/package authority until Nimbus owns a Bun resolver policy. |
| Generated package maps | `src/embed_probe/nimbus_generated_program_bundle.js` contains `nodeBuiltinModule` and `nodeExternalPackage` helper functions. | Empty generated maps deny missing Node builtins and external packages by default, which is the right generated-wrapper posture. |
| Existing V8/Deno Node resolver | `crates/nimbus-runtime/src/module_loader.rs` and `crates/nimbus-runtime/src/node_compat.rs` own the Deno/V8 resolver lane. | Bun/JSC must not reuse `node_external_packages` as if Bun and Deno/V8 had the same resolver semantics. |

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
nimbus bun embed package/module policy:
  artifact_shape: self_contained_program_wrapper
  evaluation_format: program_via_Bun__REPL__evaluate
  static_esm_import_in_program: rejected
  dynamic_import_node_fs: unsafe_import_fulfilled
  require: absent_by_default
  Bun.resolve: unsafe_bypass
  Bun.resolveSync: unsafe_bypass
  generated_node_builtin_empty_map: denied_by_generated_wrapper
  generated_external_package_empty_map: denied_by_generated_wrapper
  selected_next_lane: program_wrapper
  required_resolver_api: nimbus_owned_bun_package_resolver
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

Status: package/module policy measured, containment not proven.

The selected next artifact lane remains the generated self-contained program
wrapper. That lane lets Nimbus avoid Bun's ESM module loader while continuing
to prove host-call, timeout, memory, and lifecycle behavior against the real
generated invocation wrapper.

The generated wrapper already has a good default-deny shape for missing Node
builtin and external-package maps. However, dynamic `import("node:fs")` fulfilled
and Bun resolver APIs are exposed. A production Bun/JSC backend therefore needs
a first-class Nimbus-owned Bun package resolver and import policy before any
tenant-controlled package loading can run.

Bun/JSC remains `in_process_trusted_only` and proof-only. The next gate should
measure lifecycle, retained reuse, teardown, cancel/drop loops, and discard
policy.
