# Bun/JSC Gate 8: Artifact Shape Decision

Date: 2026-05-21

Nimbus base revision: `8037ce9c` (`Record Bun program bundle proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun proof revision: `08b993bce8` (`Add Bun embed program bundle proof`)

Bun worktree status: clean during this decision gate.

## Question

Does the Bun/JSC proof path require Nimbus to prove Bun ESM module loading
next, or can Nimbus generate a Bun-oriented JavaScript program wrapper while
keeping the current Deno/V8 bundle as ESM?

## Source Audit

The current generated Convex runtime bundle is an ESM module:

- `packages/codegen/src/emit/runtime_bundle.mjs` builds an optional static
  import preamble for Node runtime bindings.
- `packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs` ends
  the generated bundle with `export {};`.
- `crates/nimbus-runtime/src/runtime/driver/loading.rs` loads the bundle with
  `load_main_es_module(...)`, evaluates it, then invokes
  `globalThis.__nimbusInvoke(...)` through a later script evaluation.

After ESM evaluation, however, the invocation contract is program-shaped:

- the bundle installs `globalThis.__nimbusInvoke`,
- generated runtime handlers use `new Function(...)` plus materialized binding
  values,
- host integration flows through globals such as `__nimbusCreateContext` and
  `__nimbusAsyncHostValue`,
- the runtime host invokes the installed global function by request JSON.

That means Bun/JSC does not need to reuse the V8 ESM loader shape to prove the
next bundle gate. It does need an explicit JavaScript evaluation-format axis
before production routing, because `javascript` alone does not distinguish an
ESM module from a self-contained program wrapper.

## Proof Shape

This gate adds an internal codegen proof emitter, not a production Bun backend:

- the existing `generateRuntimeBundle(...)` path remains ESM and still emits
  the import preamble plus `export {};`;
- the new `generateRuntimeProgramBundle(...)` path emits the same runtime
  preamble, execution helpers, dispatch, and `__nimbusInvoke` global without
  top-level `import` or `export` syntax;
- the program emitter rejects manifests that require Node builtin or external
  package imports, because those still require a loader or Bun-specific package
  resolution proof.

The selftest uses a real generated Convex manifest with runtime-only mutations
and generated scheduled-function references. It creates the program bundle,
checks that module-only syntax is absent, evaluates it in an isolated JavaScript
context, invokes `globalThis.__nimbusInvoke(...)`, and verifies the scheduled
internal mutation reference and return value.

## Verification

Codegen selftest:

```sh
npm run test --workspace @nimbus/codegen
```

Result: passed with exit status 0.

Whitespace check:

```sh
git diff --check -- \
  packages/codegen/src/emit/runtime_bundle.mjs \
  packages/codegen/src/emit/runtime_bundle_parts.mjs \
  packages/codegen/src/emit/runtime_bundle_dispatch.mjs \
  packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs \
  packages/codegen/src/selftest/runtime_fixtures.mjs \
  docs/architecture/runtime/engine-seam.md \
  docs/architecture/runtime/new-engine-proof-harness.md \
  docs/plans/proof/runtime-engine/bun-jsc/gate-8-artifact-shape.md \
  docs/plans/runtime-engine-seam-plan.md
```

Result: passed with no whitespace errors.

## Decision

Status: program-wrapper artifact shape selected for the next Bun/JSC proof
lane.

Bun/JSC should continue with a self-contained program-wrapper artifact for the
near-term in-process proof. That shape matches the part of the real Nimbus
bundle contract that survives after V8 module evaluation: install an invocation
global, call the engine-neutral Nimbus context contract, and settle the returned
promise or value.

The existing Deno/V8 lane remains ESM. This gate does not change production
runtime routing and does not make Bun/JSC selectable.

The explicit blockers before promotion remain:

- Bun ESM/module loading is still unproven in the bare embed path because Gate
  5 hit a `JSModuleLoader::evaluate(...)` failure before host-call execution.
- Node builtin and external package descriptors are rejected by the program
  emitter until Bun has an explicit package-resolution lane.
- Runtime metadata still needs a production field for JavaScript evaluation
  format before a Bun-backed bundle can be routed honestly.
- Timeout/cancel, permission containment, memory policy, VM reuse/teardown, and
  reproducible build integration remain open proof gates.

The next useful proof is to feed the real generated program-wrapper source into
the Bun embed target, replacing the synthetic Gate 7 bundle while preserving
the same host-call assertions. A timeout/cancel gate can run after that, but
using a real generated artifact first will make later cancellation and teardown
evidence more representative.
