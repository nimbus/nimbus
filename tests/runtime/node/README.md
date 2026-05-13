# Node.js Runtime Support Evidence Roots

This tree holds the repo-owned trust artifacts that sit beside the vendored
upstream Node fixture corpus:

- `canary-registry.json`
  - package/framework claim map for pinned runtime-preset canaries
- `networking-canaries/`
  - current `Application`-preset networking canary root
- `tooling-canaries/`
  - current `Tooling`-preset package canary root for `tsx`, `ts-node`,
    `jest`, `prisma`, and `next`
- `expectations/`
  - checked-in expected-failure/watchpoint catalogs validated against the Rust
    harness inventory
- `schemas/`
  - dependency-free JSON schemas for expectation catalogs, sync reports,
    refresh reports, and trend snapshots

Canonical developer entrypoints:

```bash
make node-compat-report FAMILY=networking SLICE=dns-net-foundation CAPTURE_LIVE=1
make node-compat-report FAMILY=loader-context-supplementary SLICE=supplementary-builtin-completeness CAPTURE_LIVE=1
make node-compat-report FAMILY=loader-context-supplementary-module-bridge SLICE=supplementary-module-resolution-bridge CAPTURE_LIVE=1
make node-compat-report FAMILY=loader-context-supplementary-global-injection SLICE=supplementary-global-injection-fidelity CAPTURE_LIVE=1
make node-compat-report FAMILY=process-and-timing-supplementary SLICE=supplementary-process-release-shape CAPTURE_LIVE=1
make node-compat-report FAMILY=runtime-supplementary SLICE=supplementary-resource-safety CAPTURE_LIVE=1
make node-compat-report FAMILY=runtime-supplementary SLICE=supplementary-framework-loader-patterns CAPTURE_LIVE=1
make node-compat-report FAMILY=runtime-supplementary-signal-lifecycle SLICE=supplementary-signal-listener-lifecycle CAPTURE_LIVE=1
make node-compat-canaries-bootstrap PRESET=application
make node-compat-canaries PRESET=application
make node-compat-canaries-bootstrap PRESET=tooling
make node-compat-canaries PRESET=tooling
make node-compat-oracle LANE=node22 SAMPLE=test/parallel/test-buffer-alloc.js
make node-compat-validate-watchpoints
make node-compat-sync LANE=node22 DRY_RUN=1
make node-compat-refresh LANE=node22 TAG=v22.15.0 DRY_RUN=1
make node-compat-status
make node-compat-dashboard
make node-compat-trends
make node-compat-publish-evidence
```

The nightly evidence workflow in `.github/workflows/node-compat-nightly.yml`
replays the representative Node test checks, both canary presets, and a
version-matched Node20 / Node22 / Node24 oracle sample sweep before emitting
the retained dashboard bundle.

Generated evidence lands under `target/node-compat/`, including:

- representative Node test check reports under `target/node-compat/<family>/<slice>/`
- canary reports under `target/node-compat/canaries/`
- oracle artifacts under `target/node-compat/oracle/`
- suite-wide status summaries under `target/node-compat/status/`
- dry-run fixture sync plans under `target/node-compat/sync/`
- aggregated dashboard summaries under `target/node-compat/dashboard/`
- trend snapshots under `target/node-compat/trends/`
- curated publish snapshots under
  `docs/architecture/runtime/node-compat-evidence/latest/` or a custom
  `PUBLISH_ROOT`

All generated Node test check, canary, oracle, and dashboard summaries now carry the
lane-separation metadata needed for successor-scope trust work:
`upstream_fixture_line`, `lane_role`, `public_contract_role`,
`runtime_execution_target`, and `runtime_limits_preset`.

`make node-compat-status` is the truthful suite-wide denominator surface. It
counts every lane-local vendored `test-*` JS/CJS/MJS fixture and compares that
denominator to the documented green manifested subset plus explicit lane
classification catalogs. The Node22 default lane uses reconstructable
path-owned fixture evidence as its green numerator when prose family counts and
path evidence disagree. Anything outside the manifested green denominator and
classification catalogs is reported as `unmanifested_or_unclassified`, not as
pass or fail. The same report validates
`tests/runtime/node/expectations/rust-watchpoints.json` against the current Rust
`#[ignore]` inventory and can fail on unexpected passes when an observed-results
JSON file is provided through `OBSERVED_RESULTS`.

`make node-compat-sync`, `make node-compat-refresh`, `make node-compat-trends`,
and `make node-compat-validate-watchpoints` validate their JSON outputs or
catalogs against the checked-in schemas in `tests/runtime/node/schemas/`.
`make node-compat-trends` compares the current generated status/dashboard
against the checked-in latest evidence snapshot before `make
node-compat-publish-evidence` refreshes that snapshot.

Supplementary Node test check reports also carry:

- `test_tier`
- `supplementary_category`

Current checked-in supplementary outcomes:

- `loader-context-supplementary:supplementary-builtin-completeness`
  - measured green across `node20`, `node22`, and `node24`
- `loader-context-supplementary-module-bridge:supplementary-module-resolution-bridge`
  - measured green across `node20`, `node22`, and `node24`
- `loader-context-supplementary-global-injection:supplementary-global-injection-fidelity`
  - measured green across `node20`, `node22`, and `node24`
- `process-and-timing-supplementary:supplementary-process-release-shape`
  - measured expected-failure check; currently fails on all three lanes because
    the runtime still exposes a Node22-shaped `process.version` surface and
    omits the expected Node22 `process.release.lts` label
- `runtime-supplementary:supplementary-resource-safety`
  - measured green across `node20`, `node22`, and `node24`
- `runtime-supplementary:supplementary-framework-loader-patterns`
  - measured green across `node20`, `node22`, and `node24`
- `runtime-supplementary-signal-lifecycle:supplementary-signal-listener-lifecycle`
  - measured expected-failure check; currently fails on all three lanes because
    `process.on('SIGINT', ...)` reaches unavailable `Deno.addSignalListener`
