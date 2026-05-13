# Node Compatibility Surface Matrix

Status: generated-evidence index

This page is the architecture-facing index for Nimbus's Node compatibility
support posture. The detailed support matrix is generated from checked-in
manifest and evidence artifacts instead of being copied into this hand-written
document.

## Source Of Truth

Use these generated artifacts for current support numbers and lane details:

- [`docs/runtimes/nodejs/evidence/latest.md`](../../runtimes/nodejs/evidence/latest.md)
- [`docs/runtimes/nodejs/evidence/node20.md`](../../runtimes/nodejs/evidence/node20.md)
- [`docs/runtimes/nodejs/evidence/node22.md`](../../runtimes/nodejs/evidence/node22.md)
- [`docs/runtimes/nodejs/evidence/node24.md`](../../runtimes/nodejs/evidence/node24.md)
- [`node-compat-evidence/latest/status-summary.md`](node-compat-evidence/latest/status-summary.md)
- [`node-compat-evidence/latest/dashboard-summary.md`](node-compat-evidence/latest/dashboard-summary.md)
- [`node-compat-evidence/latest/trend-summary.md`](node-compat-evidence/latest/trend-summary.md)

The generator is `scripts/runtime/node/publish_docs.py`; run
`make node-compat-publish-docs CHECK=1` to verify that the checked-in public
evidence pages match the current architecture evidence snapshots.

## Manifest Inputs

Manifest-owned fixture families live under
`crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/fixtures/`.
Lane metadata lives under
`crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/`.

The broad Node LTS source baseline remains generated separately:

- [`node-lts-compat-summary.md`](node-lts-compat/node-lts-compat-summary.md)
- [`node-lts-compat-matrix.csv`](node-lts-compat/node-lts-compat-matrix.csv)
- [`node20-symbols.csv`](node-lts-compat/node20-symbols.csv)
- [`node22-symbols.csv`](node-lts-compat/node22-symbols.csv)
- [`deno-node-impl-inventory.csv`](node-lts-compat/deno-node-impl-inventory.csv)

## Public Contract

- Node22 is the default compatibility target.
- Node20 and Node24 are supported selectable targets.
- Nimbus does not claim full Node built-in compatibility for any target.
- A surface is supported only when a passed fixture, canary, oracle check, or
  explicit classification supports that claim.
- Expected failures, known gaps, skips, exclusions, and unclassified fixtures
  are not support claims.
- Runtime permission mode and explicit grants remain separate from Node
  compatibility target selection.

## Runtime Posture

The generated evidence currently proves a bounded Node-compatible runtime
surface for Convex-compatible `"use node"` actions. It should be read as an
explicit, measured contract rather than an implication of Node CLI parity.

Host-heavy behavior such as `node --test`, inspector behavior, workers, child
processes, native addons, and filesystem or network access is supported only
where the generated evidence names the specific fixture, canary, or
classification.

## Refresh Workflow

Use [`docs/runtimes/nodejs/evidence/refreshing.md`](../../runtimes/nodejs/evidence/refreshing.md)
when updating lane metadata, syncing against an upstream Node tag, regenerating
dashboards, or preparing a future `nodeNN` lane.
