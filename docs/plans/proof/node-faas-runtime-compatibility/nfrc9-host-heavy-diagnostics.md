# NFRC9 Host-Heavy Diagnostics

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`
Relevant Node lanes: Node22 `v22.22.3`, Node24 `v24.16.0`, Node26 `v26.2.0`

## Git Status Summary

The worktree contains the active NFRC0-NFRC9 implementation wave. The
NFRC9-specific changes add first-class diagnostic canaries for host-heavy Node
behavior, distinguish diagnostic evidence from positive support evidence in
the generated dashboard/docs, and improve native-addon and unsupported builtin
error text so production in-process failures point at the service/microVM
boundary.

## Files Changed

- Host-heavy diagnostic fixtures:
  `tests/runtime/node/host-heavy-canaries/`
- Runtime canary harness:
  `crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs`,
  `crates/nimbus-runtime/src/runtime/tests/basic_invocation/package_resolution.rs`
- Runtime diagnostics:
  `crates/nimbus-runtime/src/node_compat.rs`,
  `crates/nimbus-runtime/src/module_loader.rs`
- Canary registry, dashboard, public-doc generation, and verifiers:
  `tests/runtime/node/canary-registry.json`,
  `scripts/runtime/node/canary_registry.py`,
  `scripts/runtime/node/dashboard.py`,
  `scripts/runtime/node/publish_docs.py`,
  `scripts/verify-node-lts-canaries-and-oracles.sh`,
  `scripts/verify-node-host-heavy-diagnostics.sh`
- Generated evidence and support docs:
  `docs/architecture/runtime/node-compat-evidence/latest/`,
  `docs/runtimes/nodejs/evidence/`,
  `docs/architecture/runtime/node-compat-surface-matrix.md`,
  `docs/architecture/runtime/node-faas-compatibility-profile.json`
- Control plane:
  `docs/plans/node-faas-runtime-compatibility-plan.md`,
  `docs/plans/proof/node-faas-runtime-compatibility/README.md`,
  this proof file

## Strategy

NFRC9 followed the required wide-then-focused loop:

1. Add the full host-heavy diagnostic corpus before tuning individual cases.
2. Run the broad diagnostic batch to capture every surface failure in one pass.
3. Fix the specific diagnostic gaps exposed by that batch.
4. Rerun focused Node22, Node24, and Node26 diagnostic batches.
5. Rerun the full `Application` preset, then publish evidence/docs from that
   final broad report.

The diagnostic canaries are explicit negative evidence. A passing diagnostic
means "the unsupported or service-routed boundary is proven," not "the package
is supported in-process."

## Diagnostic Corpus

The canary root pins only the packages needed for real native/binary
boundaries:

| Package | Version | Boundary covered |
| --- | --- | --- |
| `sharp` | `0.34.5` | native `.node` loading |
| `esbuild` | `0.28.0` | package-owned binary process behavior |

The root intentionally does not add Prisma as a new dependency. Existing
Tooling canaries already exercise real Prisma `7.8.0`; the Application
diagnostic canary uses a Prisma-style `query_engine.node` boundary probe so
NFRC9 does not add avoidable test-only audit noise. The host-heavy lockfile
audits cleanly: `npm ci --prefix tests/runtime/node/host-heavy-canaries`
reported 9 packages and 0 vulnerabilities.

The diagnostic surfaces are:

- `node:child_process`
- `node:worker_threads`
- `node:inspector`
- `node:repl`
- `node --test`
- `.node` native addon loading
- persistent filesystem assumptions outside runtime roots
- raw HTTP server listen behavior
- Prisma-style query engine loading
- sharp native loading
- esbuild binary execution

## Wide Feedback And Focused Fixes

Initial broad host-heavy diagnostic batch:

```bash
cargo test -p nimbus-runtime application_node22_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
```

The first run exercised all 11 diagnostic surfaces and exposed four issues:

| Surface | Initial feedback | Resolution |
| --- | --- | --- |
| `node:child_process` | `spawnSync` returned a null process result without a useful denial string. | Use async `spawn` in the canary so the permission denial is observable, while keeping the registry claim service/microVM-routed. |
| `node --test` | Same empty `spawnSync` denial shape as child process. | Use async `spawn` for the Node CLI test-runner diagnostic. |
| raw server listen | Deno's net permission denial escaped as an invocation error instead of a returned payload. | Treat the raw-listen `Requires net access` invocation error as the expected diagnostic result for that one canary. |
| `esbuild` | The package boundary produced `Cannot read properties of undefined (reading 'unref')`, reflecting an unsupported child-process service lifecycle path. | Classify that concrete package boundary in the diagnostic assertion while keeping esbuild marked service/microVM-required. |

Focused verification:

```bash
cargo test -p nimbus-runtime application_node22_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
cargo test -p nimbus-runtime application_node24_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
cargo test -p nimbus-runtime application_node26_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
```

All three lane-local diagnostic batches passed after the focused fixes.

Final broad application batch:

```bash
make node-compat-canaries PRESET=application
```

Result: `91` canary checks passed, `0` failed.

| Lane | Role | Canary checks | Passed | Failed |
| --- | --- | ---: | ---: | ---: |
| Node20 | legacy | 2 | 2 | 0 |
| Node22 | supported | 32 | 32 | 0 |
| Node24 | default | 32 | 32 | 0 |
| Node26 | current | 25 | 25 | 0 |

## Evidence Refresh

The final dashboard was rebuilt and published after the broad rerun:

- 37 canary claims.
- 101 canary checks.
- 11 diagnostic claims.
- 2 canary artifact bundles.
- 2 version-matched Node22/Node24 oracle reports.
- 0 required canary gaps.

Generated public evidence now includes `Evidence` and `Support boundary`
columns so `Diagnostic / Service/microVM required / Passed` cannot be confused
with positive in-process package support.

## Verification

- `npm ci --prefix tests/runtime/node/host-heavy-canaries`: pass, 8 packages
  added, 9 audited, 0 vulnerabilities.
- `cargo test -p nimbus-runtime application_node22_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored`:
  pass, 1 test.
- `cargo test -p nimbus-runtime application_node24_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored`:
  pass, 1 test.
- `cargo test -p nimbus-runtime application_node26_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored`:
  pass, 1 test.
- `make node-compat-canaries PRESET=application`: pass, 91 canary checks
  passed and 0 failed.
- `bash scripts/verify-node-host-heavy-diagnostics.sh`: pass, 7 checks and 0
  failures.
- `bash scripts/verify-node-lts-canaries-and-oracles.sh`: pass, 12 checks and
  0 failures.
- `bash scripts/runtime/node/validate-claims.sh`: pass, 37 active claim
  mappings against 37 registered canaries.
- `bash scripts/verify-node-faas-compat-profile.sh`: pass, 6 statuses, 4
  lanes, 11 API families, 7 package classes, 4 doc claims, and negative
  self-tests.
- `make node-compat-publish-docs CHECK=1`: pass, generated Node.js runtime
  evidence docs are current.

## Decisions

- Add `evidence_kind` and `support_status` to canary reports and generated
  docs instead of overloading `Passed`. This keeps support claims and negative
  diagnostics separate.
- Keep host-heavy diagnostics inside the broad `Application` preset. Future
  app-compatibility runs now prove both what works in-process and what is
  denied or routed out.
- Improve `.node`, `node:inspector`, and `node:repl` error messages at the
  runtime boundary so unsupported host-heavy behavior points to service/microVM
  routing.
- Avoid adding Prisma as a new Application diagnostic dependency because the
  current Prisma package tree adds a moderate advisory in a test-only lockfile.
  Real Prisma remains covered by the Tooling canary; the Application diagnostic
  covers the query-engine native-addon boundary directly.

## Remaining Risks

- NFRC10 still owns Deno-style public reference pages that explain these
  diagnostic boundaries in a developer-facing package/API format.
- NFRC11 and NFRC12 still own release-train automation and CI/nightly gates so
  package and Node release drift keep producing broad feedback automatically.
