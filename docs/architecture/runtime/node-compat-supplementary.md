# Node Compatibility Supplementary Evidence

This document tracks Neovex-authored supplementary Node-compatibility probes
that sit between the vendored upstream Node test corpus and the package or
framework canary layer.

Current measured slices:

- `supplementary-builtin-completeness`
  - Category: `builtin_completeness`
  - Scope: bare builtin import and require coverage for `fs` and `path`,
    including `node:` forms, `createRequire(...)`, and
    `process.getBuiltinModule(...)` when present
  - Measured outcome: green on `node20`, `node22`, and `node24`
- `supplementary-module-resolution-bridge`
  - Category: `module_resolution_bridge`
  - Scope: bare-specifier conditional-exports resolution for ESM `import` and
    `createRequire(...)`, plus ESM loading of a CommonJS leaf
  - Measured outcome: green on `node20`, `node22`, and `node24`
- `supplementary-global-injection-fidelity`
  - Category: `global_injection_fidelity`
  - Scope: CJS `require` / `__dirname` / `__filename` injection plus the
    corresponding absence of those globals in ESM modules
  - Measured outcome: green on `node20`, `node22`, and `node24`
- `supplementary-process-release-shape`
  - Category: `process_object_shape`
  - Scope: lane-specific `process.version`, `process.versions.node`, and
    `process.release.lts` shape for the carried Node20, Node22, and Node24
    lines
  - Measured outcome:
    - `node20`: expected failure, still reports `v22.0.0-neovex` instead of a Node20 line
    - `node22`: expected failure, still omits the expected `process.release.lts` label
    - `node24`: expected failure, still reports `v22.0.0-neovex` instead of a Node24 line
- `supplementary-resource-safety`
  - Category: `resource_safety`
  - Scope: file-handle close/use-after-close behavior, abortable
    `node:timers/promises`, and bundle-root cleanup
  - Measured outcome: green on `node20`, `node22`, and `node24`
- `supplementary-framework-loader-patterns`
  - Category: `framework_motivated_patterns`
  - Scope: CommonJS `require.extensions` custom loader registration/restoration,
    require cache visibility, and package `main` resolution for toolchain-style
    loaders
  - Measured outcome: green on `node20`, `node22`, and `node24`
- `supplementary-signal-listener-lifecycle`
  - Category: `resource_safety`
  - Scope: `process.on/off` listener lifecycle for real POSIX signal names
    without sending a host signal
  - Measured outcome: expected failure on `node20`, `node22`, and `node24`;
    the embedded runtime currently reaches `Deno.addSignalListener`, which is
    unavailable in this host path

These probes are successor-scope correctness evidence. They do not widen the
completed `NLC` support denominator by themselves.
