# Bun/JSC Gate 19: Resolver And Package Lockdown Decision

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-in-process-lockdown-plan.md`

Inputs:

- `docs/plans/proof/runtime-engine/bun-jsc/gate-13-package-module-policy.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-18-in-process-lockdown-source-map.md`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun local proof head: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

## Decision

Status: source-blocked for `in_process_untrusted`.

Keep the selected artifact shape as a self-contained generated program wrapper,
but do not claim resolver/package lockdown for Bun/JSC. The current non-CLI
embed lane has useful construction properties, but Bun still owns dynamic
import and resolver authority below Nimbus' generated wrapper.

The next viable implementation path is a Bun embedder resolver policy API. The
policy must run before dynamic import, `Bun.resolve*`, `import.meta.resolve`,
CommonJS, Node builtin, package-root, plugin, and native-addon resolution can
touch the host filesystem or Bun's builtin/module registries.

## Current Proof Result

Gate 13 measured the current package/module state:

| Surface | Result | Product implication |
| --- | --- | --- |
| Artifact shape | `self_contained_program_wrapper` | Keep this as the first proof lane because it avoids relying on Bun's ESM entrypoint for Nimbus-generated code. |
| Evaluation format | `program_via_Bun__REPL__evaluate` | Acceptable for proof code and generated wrappers only. |
| Static ESM import in program evaluator | `rejected` | Good for the selected wrapper lane. |
| Dynamic `import("node:fs")` | `unsafe_import_fulfilled` | Blocker. Dynamic import bypasses Nimbus generated helper maps and reaches Bun module loading. |
| `require` | `absent_by_default` | Good only while Nimbus never calls `Bun__REPL__setupGlobalRequire`. |
| `Bun.resolve` | `unsafe_bypass` | Blocker. Present global resolver API exposes host/package resolution. |
| `Bun.resolveSync` | `unsafe_bypass` | Blocker. Present sync resolver API exposes host/package resolution. |
| Generated Node builtin empty map | `denied_by_generated_wrapper` | Good for Nimbus helper calls, but not a Bun loader boundary. |
| Generated external package empty map | `denied_by_generated_wrapper` | Good for Nimbus helper calls, but not a Bun loader boundary. |

This is enough to close `BIL3` as a blocker rather than an implementation
proof: the selected lane can keep `require` absent and generated wrapper maps
deny by default, but dynamic import and resolver APIs remain outside Nimbus
policy.

## Source-Level Blockers

| Resolver lane | Source owner | Why it blocks `in_process_untrusted` |
| --- | --- | --- |
| Dynamic import | `src/jsc/bindings/ZigGlobalObject.cpp` `GlobalObject::moduleLoaderImportModule` resolves module names through `Zig__GlobalObject__resolve` and then calls `JSC::importModule`. | There is no Nimbus policy decision before the Bun resolver can fulfill `import("node:fs")`. |
| Module-loader resolve | `src/jsc/bindings/ZigGlobalObject.cpp` `GlobalObject::moduleLoaderResolve` also calls `Zig__GlobalObject__resolve` and consults plugin virtual modules. | Static and dynamic module graph resolution can traverse Bun plugin/module state before Nimbus approval. |
| Bun resolver API | `src/runtime/api/BunObject.rs` exposes `BunObject_callback_resolve` and `BunObject_callback_resolveSync`, then calls `do_resolve` / `do_resolve_with_args`. | The public `Bun.resolve*` APIs are present and route directly into Bun resolver behavior. |
| C ABI resolver exports | `src/runtime/api/BunObject.rs` exports `bun_resolve`, `bun_resolve_sync`, `bun_resolve_sync_with_paths`, `bun_resolve_sync_with_strings`, and `bun_resolve_sync_with_source`. | Import-meta, CommonJS, and C++ module loader paths can invoke the resolver below the JavaScript wrapper surface. |
| Resolver dispatch | `src/runtime/jsc_hooks.rs` documents the shared resolution path behind `Bun__resolveSync`, `Zig__GlobalObject__resolve`, `import.meta.resolve`, and `Module._findPath`. | A single shared resolver hook is exactly where Nimbus policy should live; today it has no Nimbus policy input. |
| `import.meta.resolve` | `src/jsc/bindings/ImportMetaObject.cpp` exposes `resolve`, `resolveSync`, and `require` properties and calls `Bun__resolveSync*`. | Hiding `Bun.resolve*` is insufficient if `import.meta.resolve*` remains installed. |
| CommonJS `require` | `src/jsc/bindings/ExposeNodeModuleGlobals.cpp` installs global `require` only through `Bun__REPL__setupGlobalRequire`; `src/js/builtins/CommonJS.ts` owns require semantics. | This lane is safe only because Nimbus does not call setup. Enabling CommonJS later needs the same resolver policy. |
| Plugins and virtual modules | `src/jsc/bindings/ZigGlobalObject.cpp` consults `onLoadPlugins.resolveVirtualModule`; `src/jsc/bindings/BunPlugin.*` and `src/bundler_jsc/PluginRunner.*` own plugin registration/execution. | Plugin-created module identities and virtual modules can affect resolution before host policy. |
| Native addons | `src/runtime/napi/*`, `src/runtime/ffi/*`, and resolver hooks around embedded node files in `src/runtime/jsc_hooks.rs` own native loading lanes. | Resolver policy must be able to deny native addons and dynamic libraries even if package loading is allowed later. |

## Required Bun Embedder Resolver Policy

The policy API Nimbus needs should be explicit and synchronous at resolver
entrypoints:

```text
BunEmbedderResolverPolicy::resolve(request) -> decision

request:
  specifier
  referrer
  import_kind:
    static_esm | dynamic_import | bun_resolve | import_meta_resolve |
    commonjs_require | require_resolve | node_builtin | native_addon |
    plugin_virtual
  asserted_type / import attributes
  package_root_candidate
  resolved_path_candidate
  tenant/runtime identity projection

decision:
  deny(reason, audit_code)
  allow_builtin(canonical_builtin_id)
  allow_generated_bundle_path(path)
  allow_package_path(path, package_id, digest/evidence)
  allow_virtual_module(namespace, id)
```

The first `in_process_untrusted` profile should use this policy in deny-by-
default mode:

- allow only Nimbus-generated wrapper code and its internal helper maps
- deny all Node builtins unless explicitly generated by Nimbus for this backend
- deny all external package roots
- deny all native addons
- deny all plugins and virtual modules
- deny `Bun.resolve*` and `import.meta.resolve*` unless the request is scoped
  to an allowed generated bundle path
- deny dynamic import by default

## Why Nimbus Wrapper Maps Are Not Enough

The generated wrapper helpers are still useful:

- `nodeBuiltinModule("node:fs")` denies when `__nimbusNodeBuiltinModules` lacks
  an entry.
- `nodeExternalPackage("left-pad")` denies when
  `__nimbusNodeExternalPackages` lacks an entry.

Those helpers only protect code that routes through the generated Nimbus
bundle. Gate 13 proved tenant code can use dynamic import syntax directly, and
Gate 18 mapped `Bun.resolve*` / module-loader paths below those helpers.

Therefore, wrapper-only lockdown cannot satisfy `in_process_untrusted`.

## Verification Evidence

Gate 13 native proof output included:

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
```

Read-only source verification was performed against the local Bun worktree
with `rg` over:

- `src/runtime/api/BunObject.{rs,zig}`
- `src/runtime/jsc_hooks.rs`
- `src/jsc/bindings/ImportMetaObject.cpp`
- `src/jsc/bindings/ExposeNodeModuleGlobals.cpp`
- `src/js/builtins/CommonJS.ts`
- `src/jsc/bindings/ZigGlobalObject.cpp`
- `src/jsc/bindings/ModuleLoader.cpp`
- `src/jsc/JSModuleLoader.{rs,zig}`

No Bun files were modified for this gate.

## Outcome

`BIL3` is complete as a source-blocked decision. Bun/JSC remains
`in_process_trusted_only` until an upstream or forked embedder resolver policy
can deny or mediate every resolver lane named above.
