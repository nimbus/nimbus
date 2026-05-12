# Convex Node Bundling And External Packages Plan

Status: done
Owner: Convex adapter, codegen, and Node-compatible runtime

## Purpose

Close the gap between Neovex's Convex-compatible Node action runtime and the
Convex bundling contract in a way that is useful, explicit, and evidence-backed.
Convex bundles every `convex/` module for either the default runtime or the
Node.js runtime, lets Node actions opt packages out of bundling through
`convex.json` `node.externalPackages`, supports `["*"]` as "externalize all
packages used by Node actions", and derives external package versions from the
local `node_modules` tree.

Neovex should support the same authoring shape where it can do so truthfully.
Where cloud-style behavior is not implemented yet, codegen and runtime startup
must fail with precise diagnostics rather than silently producing a bundle that
only works by accident.

Primary sources:

- Convex runtimes docs: https://docs.convex.dev/functions/runtimes
- Convex project configuration docs:
  https://docs.convex.dev/production/project-configuration
- Convex bundling docs: https://docs.convex.dev/functions/bundling

## Guardrails

- Keep `"use node"` restricted to action-only modules.
- Keep default-runtime modules fail-closed for Node builtin imports, with
  `--debug-node-apis` preserving import-chain diagnostics.
- Do not claim full Convex cloud package install parity until Neovex can stage
  or install the external package payload deterministically.
- Treat `node.externalPackages` as Node-action-only configuration.
- Keep reusable parsing/resolution mechanics provider-neutral where practical;
  keep Convex-specific config, diagnostics, and artifact shape under Convex
  codegen ownership so Firebase, Cloud Functions, and future adapters do not
  inherit Convex policy by accident.
- Make package diagnostics actionable: include the package/import specifier,
  source module, and the exact config change or install step needed.
- Preserve local development ergonomics: packages installed in the project
  `node_modules` should be usable by Node actions when explicitly externalized.

## Work Queue

### CNB1 Source Contract And Plan Baseline

- [x] Re-read the Convex runtime, project configuration, and bundling docs.
- [x] Create this active plan and link it from the plan index.
- [x] Document the current Neovex support boundary in Convex compatibility docs.

### CNB2 External Package Config Semantics

- [x] Validate `node.externalPackages` as either explicit package specifiers or
  exactly `["*"]`.
- [x] Normalize package matching so `@scope/pkg/subpath` matches `@scope/pkg`.
- [x] Fail Node action package imports that would require unimplemented bundled
  package semantics unless they are externalized.
- [x] Resolve configured external packages from local `node_modules` and fail
  precisely when the package is missing.

### CNB3 Runtime Bundle Materialization

- [x] Preserve static package imports used by Node action handlers when handler
  source is extracted into generated runtime functions.
- [x] Generate runtime binding descriptors for external package namespace,
  default, and named imports.
- [x] Keep builtin imports and external package imports separate in generated
  preambles for clearer diagnostics.

### CNB4 Evidence Metadata And Size Reporting

- [x] Emit a checked-in-shape generated metadata artifact for external package
  mode, resolved package paths, package roots, source modules, import specifiers,
  and package directory sizes.
- [x] Record bundle-size/external-package-size limits separately from Convex's
  cloud limits until Neovex enforces the same thresholds.

### CNB5 Runtime/Deploy Validation

- [x] Ensure runtime startup errors identify missing generated package bindings
  distinctly from missing Node builtin bindings.
- [x] Add server-side manifest validation for external package metadata once the
  codegen artifact is stable.
- [x] Decide whether Neovex should copy/stage external package payloads under
  `.neovex/convex/` or intentionally depend on project-local `node_modules` for
  local development.

### CNB6 Docs, Tests, And Closeout

- [x] Add codegen selftests for explicit `node.externalPackages`, `["*"]`,
  missing local installs, unexternalized package imports, scoped packages, and
  generated runtime bindings.
- [x] Update Convex compatibility and CLI docs with truthful support status.
- [x] Run focused JS verification plus relevant Rust manifest/runtime checks.
- [x] Move this plan to the stable baseline section only after the support claim
  is evidence-backed and no completion-gate TODOs remain.
