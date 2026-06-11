# Node Compatibility Supplementary Failure Inventory

Current state: one active signal-listener watchpoint is carried in the
checked-in baseline.

Green slice:

- `supplementary-builtin-completeness`
- `supplementary-module-resolution-bridge`
- `supplementary-global-injection-fidelity`
- `supplementary-process-release-shape`

Configured slice pending Nimbus runtime verification:

- none

Green runtime supplementary slice:

- `supplementary-resource-safety`
- `supplementary-framework-loader-patterns`

Active measured failure slice:

- `supplementary-signal-listener-lifecycle`
  - `node20`: `process.on('SIGINT', ...)` reaches unavailable
    `Deno.addSignalListener`
  - `node22`: `process.on('SIGINT', ...)` reaches unavailable
    `Deno.addSignalListener`
  - `node24`: `process.on('SIGINT', ...)` reaches unavailable
    `Deno.addSignalListener`

If a future successor probe fails, record the owner seam and measured lane
impact here instead of folding it back into the completed `Node compatibility` family
inventories.
