# NLRT7 Harness Timeouts And Hangs

Date: 2026-05-28
Agent: Codex

## Git Status Summary

- Working tree contained NLRT7 harness/evidence/doc updates plus the pre-existing
  unrelated `docs/plans/dynamodb-adapter-plan.md` dirty file, which was not
  touched for NLRT7 staging.
- Baseline before this row: NLRT6 commit `0dfcb98c` (`Record Node fixture
  provenance`).

## Files Changed

- `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`
- `scripts/runtime/node/report.sh`
- `scripts/verify-node-compat-harness-hardening.sh`
- `docs/architecture/runtime/node-lts-compat/harness-timeouts-and-hangs.md`
- `docs/architecture/runtime/node-lts-compat/node-lts-compat-summary.md`
- `tests/runtime/node/expectations/rust-watchpoints.json`
- `tests/runtime/node/classifications/node20.json`
- `tests/runtime/node/classifications/node22.json`
- `tests/runtime/node/classifications/node24.json`
- `docs/architecture/runtime/node-compat-evidence/latest/*`
- `docs/runtimes/nodejs/evidence/*.md`
- `docs/plans/node-lts-runtime-trust-plan.md`
- `docs/plans/proof/node-lts-runtime-trust/README.md`

## Decisions

- Added an outer per-fixture harness wall-clock timeout derived from the exact
  runtime limits selected for the lane and fixture:
  `RuntimeLimits.execution_timeout + 5 seconds`.
- Kept runtime execution timeout as the primary cancellation mechanism. The new
  wall-clock guard is the harness backstop that prevents fixture execution from
  looking like a stuck Rust test process.
- Added structured diagnostic artifacts for runtime errors, non-OK payloads,
  mismatched payloads, non-zero process exits, and wall-clock timeouts.
- Classified diagnostic artifacts into `event_loop`, `vm`, `worker`,
  `message_port`, `subprocess`, or `general` so hang-prone fixture families
  have explicit exit criteria.
- Counted ignored Rust node-compat tests as watchpoints, not green support.
  `watchpoints.py sync` exposed six stale ignored watchpoints and moved the
  catalog from 61 to 67 entries; lane classifications were updated so the
  generated status still has zero unclassified fixtures.
- Kept the worker MessagePort production decision gated on NLRT8. A passing or
  ignored MessagePort fixture does not imply production in-process
  `worker_threads` authority.

## Alternatives Rejected

- Rejected publishing a partially regenerated dashboard. A local dashboard run
  without slice/canary/oracle artifacts degraded canary and oracle observations
  to missing. The full evidence bundle was rebuilt instead.
- Rejected classifying sandbox bind/DNS denials as runtime failures. The
  sandboxed networking and package canary attempts produced diagnostic artifacts
  for `EACCES`/`EPERM`; the authoritative dashboard evidence was regenerated
  from host replays for the network-dependent slices.
- Rejected leaving the worker MessagePort hazard as prose only. The
  `message_port` diagnostic family now carries a machine-written exit criterion
  and a focused test asserts it points at the NLRT8 profile split.

## Evidence Refresh

- `make node-compat-status`: regenerated `target/node-compat/status/*`.
- `make node-compat-inventory LANE=node20`, `node22`, `node24`: regenerated
  all three inventory reports.
- `make node-compat-report ... CAPTURE_LIVE=1`: regenerated all eight
  representative slice reports.
  - Seven slices are green.
  - `runtime-supplementary-signal-lifecycle:supplementary-signal-listener-lifecycle`
    remains the known measured failure with 3 failed lane observations.
- `make node-compat-report FAMILY=networking SLICE=dns-net-foundation
  CAPTURE_LIVE=1` outside the sandbox: host replay passed Node20 10/10, Node22
  10/10, and Node24 9/9.
- `make node-compat-canaries PRESET=application` outside the sandbox: passed
  Node20/Node22 application canaries and wrote `preset-application.json`.
- `make node-compat-canaries PRESET=tooling` outside the sandbox: passed the
  Node22 tooling canaries and wrote `preset-tooling.json`.
- `make node-compat-oracle LANE=node22
  SAMPLE=test/parallel/test-buffer-alloc.js
  NODE_BIN=/opt/homebrew/Cellar/node@22/22.22.2_2/bin/node`: emitted the
  Node22 oracle artifact with a passing Rust test.
- `make node-compat-dashboard`: dashboard reports 8 slice reports, 2 canary
  bundles, 12 canary checks, 1 oracle report, and 3 inventory reports.
- `make node-compat-trends`, `make node-compat-publish-evidence`, and
  `make node-compat-publish-docs`: refreshed checked-in architecture and public
  evidence from the coherent target artifact bundle.

## Verification

- `cargo test -p nimbus-runtime node_compat_harness -- --nocapture`: 3 passed,
  0 failed.
- `python3 scripts/runtime/node/watchpoints.py validate`: pass, 67 catalog
  entries with 67 matching Rust ignored watchpoints.
- `python3 scripts/runtime/node/classifications.py sync --preserve-existing
  --check`: pass, lane classifications current.
- `python3 scripts/runtime/node/status.py --output-root
  target/node-compat/nlrt7-status`: pass; zero warnings, zero unexpected
  passes, and zero unclassified fixtures for node20/node22/node24.
- `bash scripts/verify-node-compat-harness-hardening.sh`: pass; reruns the
  focused harness tests, watchpoint validation, classification check, and
  status report generation.
- `python3 scripts/runtime/node/fixture_provenance.py validate`: pass after the
  evidence refresh.
- `python3 scripts/runtime/node/publish_docs.py --check`: pass.
- `bash scripts/verify-node-lts-docs.sh`: pass.
- `bash scripts/verify-node-fixture-provenance.sh`: pass.
- `cargo fmt --all --check`: pass.
- `npm run docs:validate-refs:strict`: pass.
- `git diff --check`: pass.

## Remaining Risks

- NLRT7 does not grant production worker authority. NLRT8 must split local-dev,
  production in-process, and production service/microVM profiles and prove the
  production in-process profile excludes generic worker/listen/run authority.
- The signal-lifecycle supplementary slice remains a known measured failure
  because `Deno.addSignalListener` is unavailable in the current embedded
  surface. This row makes that failure diagnostic and visible; it does not
  widen support claims.
