# Node Compatibility Supplementary Failure Inventory

Current state: one active version-specific correctness watchpoint is carried in
the checked-in baseline.

Green slice:

- `supplementary-builtin-completeness`
- `supplementary-module-resolution-bridge`
- `supplementary-global-injection-fidelity`

Configured slice pending Neovex runtime verification:

- none

Green runtime supplementary slice:

- `supplementary-resource-safety`
- `supplementary-framework-loader-patterns`

Active measured failure slice:

- `supplementary-process-release-shape`
  - `node20`: reports `v22.0.0-neovex` instead of a Node20 version line
  - `node22`: omits the expected `process.release.lts` label
  - `node24`: reports `v22.0.0-neovex` instead of a Node24 version line
- `supplementary-signal-listener-lifecycle`
  - `node20`: `process.on('SIGINT', ...)` reaches unavailable
    `Deno.addSignalListener`
  - `node22`: `process.on('SIGINT', ...)` reaches unavailable
    `Deno.addSignalListener`
  - `node24`: `process.on('SIGINT', ...)` reaches unavailable
    `Deno.addSignalListener`

If a future successor probe fails, record the owner seam and measured lane
impact here instead of folding it back into the completed `NLC` family
inventories.
