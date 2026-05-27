# Bun/JSC Gate 30: Resolver And Package Policy Proof

Date: 2026-05-23

Nimbus plan: `docs/plans/archive/bun-jsc-embedder-api-and-pool-plan.md`

## Decision

Status: resolver/package policy hookability proven.

Bun proof commit `c5bafa6d73e341e4502fcb99e7e5c98582540090`
(`Add Bun embedder resolver denial proof`) adds a small native embedder
resolver hook and exercises it through the proof target. This is not the final
tenant-aware production evaluator. It is the proof that Bun/JSC can call a
host-owned policy boundary before module resolution escapes into Bun's normal
loader, plugin, or package machinery.

## Bun Hook Surface

The proof patch wires `Bun__embedderShouldDenyModuleResolution(...)` through
these Bun paths:

- `src/jsc/ModuleLoader.rs`: shared Rust policy hook, resolution-kind enum,
  and test-only deny-all switch for the embed proof
- `src/jsc/bindings/ZigGlobalObject.cpp`: dynamic `import(...)` module-loader
  path, checked before virtual module plugins and normal resolver work
- `src/jsc/bindings/bindings.cpp`: lower `JSModuleLoader__import` and
  `loadAndEvaluateModule` paths
- `src/runtime/api/BunObject.rs`: `Bun.resolve`, `Bun.resolveSync`,
  `require.resolve`, `import.meta.resolve`, and related resolver exports
- `src/embed_probe/lib.rs`: guarded proof assertions that reset the deny-all
  hook after the package/module probe

The production version should replace the deny-all switch with a typed Nimbus
resolver decision that receives tenant/workload identity, specifier, referrer,
resolution kind, artifact metadata, and audit context.

## Proof Results

The native embed probe now reports denial for the package and resolver paths
that matter to Nimbus containment:

```text
nimbus bun embed package/module policy:
  artifact_shape: self_contained_program_wrapper
  evaluation_format: program_via_Bun__REPL__evaluate
  static_esm_import_in_program: rejected
  dynamic_import_node_fs: denied_by_resolver_policy
  dynamic_import_package_root: denied_by_resolver_policy
  plugin_virtual_module_import: denied_by_resolver_policy
  require: absent_by_default
  Bun.resolve: denied_by_resolver_policy
  Bun.resolveSync: denied_by_resolver_policy
  native_addon_resolveSync: denied_by_resolver_policy
  generated_node_builtin_empty_map: denied_by_generated_wrapper
  generated_external_package_empty_map: denied_by_generated_wrapper
  selected_next_lane: program_wrapper
  resolver_policy_hook: native_embedder_deny_all
  required_resolver_api: nimbus_owned_bun_package_resolver
```

This covers:

- dynamic import
- `Bun.resolve*`
- CommonJS as currently exposed in the embedder profile (`require` absent)
- Node builtin resolution
- external package roots
- virtual module/plugin-style specifiers before plugin resolution
- native addon resolution through resolver APIs

## Product Implications

Nimbus should keep Bun package resolution separate from the existing
Node-compatible `node_external_packages` path. Bun/JSC needs its own resolver
policy object because Bun package resolution, plugin virtual modules,
`import.meta.resolve`, and native addon hooks are not the same ownership
surface as the current Deno/V8/Node-compatible runtime.

This gate also keeps the fork posture precise. The current local Bun delta is
small enough to be a candidate upstream embedder API request, but Nimbus should
not create a `nimbus/bun` fork until BEP6 and BEP7 prove the rest of
containment or upstream cannot provide equivalent hooks.

## Verification

Passed in `/Users/jack/src/github.com/oven-sh/bun`:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Passed in `/Users/jack/src/github.com/nimbus/nimbus`:

```sh
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

Result:

```text
Bun native check-bun-embed-probe: pass
Bun package/module policy: denied_by_resolver_policy for dynamic import,
package roots, plugin-style specifiers, Bun.resolve, Bun.resolveSync, and
native addon resolution.
Reusable Nimbus Bun/JSC gate: pass
```

## Outcome

`BEP5` is complete. The next gate is `BEP6`: prove native permission denial or
hookability for filesystem, network, env/process, subprocess, FFI, plugin,
worker, timer, fetch/WebSocket, and dynamic-code surfaces.
