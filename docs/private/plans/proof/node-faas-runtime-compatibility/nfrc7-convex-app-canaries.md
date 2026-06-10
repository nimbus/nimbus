# NFRC7 Convex Use Node App Canaries

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`
Relevant Node lanes: Node22 `v22.22.3`, Node24 `v24.16.0`, Node26 `v26.2.0`

## Git Status Summary

The worktree contains the active NFRC0-NFRC7 implementation wave, including
the already-vendored Node fixture corpora, generated Node evidence, and proof
artifacts. The NFRC7-specific changes add a real Convex `"use node"` app
canary, extend the application canary registry and dashboard gates, and fix a
nested-runtime lane-selection bug exposed by the canary.

## Files Changed

- Convex nested runtime dispatch:
  `crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/nested_runtime/dispatch.rs`
- Convex function tests:
  `crates/nimbus-server/src/tests/convex_functions.rs`,
  `crates/nimbus-server/src/tests/convex_functions/node_canaries.rs`
- Canary registry and fixture metadata:
  `tests/runtime/node/canary-registry.json`,
  `tests/runtime/node/convex-canaries/package.json`,
  `tests/runtime/node/convex-canaries/package-lock.json`
- Dashboard and verification gates:
  `scripts/runtime/node/dashboard.py`,
  `scripts/verify-node-lts-canaries-and-oracles.sh`
- Generated evidence and public docs:
  `docs/architecture/runtime/node-compat-evidence/latest/`,
  `docs/runtimes/nodejs/evidence/`,
  `docs/architecture/runtime/node-compat-surface-matrix.md`
- Control plane:
  `docs/plans/node-faas-runtime-compatibility-plan.md`,
  `docs/plans/proof/node-faas-runtime-compatibility/README.md`,
  this proof file

## Strategy

NFRC7 followed the required wide-then-focused loop:

1. Add the real Convex app canary to the broad `Application` canary preset
   instead of proving it only through isolated unit tests.
2. Run the broad batch to collect failures.
3. Use focused Convex tests to fix the concrete implementation bug and tighten
   the canary assertion shape.
4. Rerun the focused canary, then rerun the broad `Application` batch.
5. Publish the generated dashboard/docs only after the broad batch and oracle
   evidence were fresh.

## Coverage

The new `convex-use-node-real-app` canary builds a temporary Convex app bundle
with checked metadata, a staged npm package, and real Nimbus server execution.
It covers:

- public `"use node"` action invocation
- `convex-canary-package` npm package import
- `ctx.runQuery`
- `ctx.runMutation`
- intentional `ctx.runAction` runtime crossing
- `ctx.scheduler.runAfter`
- Convex value serialization
- `fetch(data:)`
- env/secret boundary checks
- Buffer, crypto, stream, path, and temporary fs behavior
- dangling promise timeout diagnostics

Node22 and Node24 are required supported-LTS lanes. Node26 is reported
separately as Current/non-LTS evidence and passed before the dashboard reported
Node26 Current canary support.

## Wide Feedback And Focused Fixes

Initial broad run:

```bash
make node-compat-canaries PRESET=application
```

The first sandboxed run failed with local listener permission errors
(`EACCES`/`PermissionDenied`) for canaries that bind loopback. This was an
environmental sandbox restriction, so the batch was rerun outside the sandbox.

The rerun exposed a real implementation bug in nested Convex runtime dispatch:
a Node action calling another runtime function through `ctx.runAction` could
enter the default `WebStandardIsolate` lane, causing imports such as
`node:buffer` to fail under the wrong runtime policy. The fix makes nested
runtime dispatch resolve `runtime_lane_for_function(name)` for the callee and
passes that lane's executor and policy into `invoke_runtime_bundle_*`.

Focused regression and canary fixes:

```bash
cargo test -p nimbus-server convex_runtime_only_action_can_run_runtime_only_mutation -- --nocapture
cargo test -p nimbus-server convex_use_node_real_app_canary_node22 -- --nocapture --test-threads=1 --ignored
cargo test -p nimbus-server convex_use_node_real_app_canary -- --nocapture --test-threads=1 --ignored
```

The focused canary also flushed out a legitimate scheduler race in the test
expectation: the scheduled write can run before the post-action query, so the
canary asserts the stable final outcome instead of relying on an incidental
intermediate count.

Final broad application batch:

```bash
make node-compat-canaries PRESET=application
```

Result: `19` canary checks passed, `0` failed.

| Lane | Role | Canary checks | Passed | Failed |
| --- | --- | ---: | ---: | ---: |
| Node20 | legacy | 2 | 2 | 0 |
| Node22 | supported | 8 | 8 | 0 |
| Node24 | default | 8 | 8 | 0 |
| Node26 | current | 1 | 1 | 0 |

## Evidence Refresh

The final dashboard was rebuilt after refreshing representative live slice
reports, canary reports, and version-matched Node22/Node24 oracle reports.

Representative live slice summary:

- 8 slice reports.
- 7 rows had `47` total passes and `0` missing observations.
- `supplementary-signal-listener-lifecycle` is intentionally recorded as 3
  expected failures with diagnostics because in-process FaaS does not expose
  process signal listener authority.

Canary/oracle dashboard summary:

- 13 canary claims.
- 29 canary checks.
- 2 oracle reports.
- 0 required canary gaps.
- No stale Node22-default or Node24-supported role labels in published evidence.

## Verification

- `cargo test -p nimbus-server convex_use_node_real_app_canary -- --nocapture --test-threads=1 --ignored`:
  pass, 3 tests; Node22, Node24, and Node26 Current real-app canaries passed.
- `make node-compat-canaries PRESET=application`: pass, 19 canary checks
  passed and 0 failed.
- `cargo test -p nimbus-server convex_runtime_only_action_can_run_runtime_only_mutation -- --nocapture`:
  pass, 1 test.
- `bash scripts/verify-node-lts-canaries-and-oracles.sh`: pass, 12 checks and
  0 failures.
- `bash scripts/runtime/node/validate-claims.sh`: pass, 13 active claim
  mappings against 13 registered canaries.
- `python3 scripts/runtime/node/fixture_provenance.py validate`: pass, 4
  vendored corpora and 2 supported LTS lanes with zero unclassified published
  results.
- `make node-compat-publish-docs CHECK=1`: pass, generated Node.js runtime
  evidence docs are current.
- `cargo fmt --all --check`: pass.
- `git diff --check`: pass.
- `npm run docs:validate-refs:strict`: pass, 226 working-tree Markdown files.

## Decisions

- Keep the new Convex app proof in the `Application` preset so future broad
  canary runs exercise it automatically.
- Treat Node26 as a real Current-line canary lane, but keep public supported
  canary claims scoped exactly to Node22 and Node24 until Node26 becomes LTS and
  passes supported-LTS promotion gates.
- Fix nested runtime dispatch at the callee-lane boundary instead of adding a
  one-off Node import exception. This preserves the runtime policy model and
  prevents future `ctx.runAction` crossings from silently borrowing the wrong
  lane.
- Keep process signal listener behavior as an expected unsupported FaaS
  authority gap with diagnostics; NFRC9 owns the broader host-heavy negative
  canary surface.

## Remaining Risks

- NFRC8 still owns broader realistic SDK/package canaries for SaaS, AI,
  payment, email, GitHub, JWT, and database HTTP clients.
- NFRC9 still owns host-heavy negative canaries and service/microVM diagnostics.
- NFRC10 still owns Deno-style generated API and package reference pages.
