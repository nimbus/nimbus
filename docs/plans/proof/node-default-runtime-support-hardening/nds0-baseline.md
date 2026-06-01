# NDS0 Baseline Proof

status: in_progress
date: 2026-06-01
branch: codex/node-default-runtime-support-hardening
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening
pr: _pending initial NDS0 scaffold commit/push_
verifier: scripts/verify-node-default-runtime-support-hardening.sh

## Row And Status

NDS0 is in progress. The dedicated worktree and branch exist, the NDS-only plan
hardening has been carried into the worktree, and the verifier/proof scaffold is
being created. NDS0 is not `done` until the branch is pushed, the draft PR or
approved substitute is recorded, and the Active Execution Pointer is updated.

## Broad Pre-Run

Baseline evidence comes from the checked-in generated Node compatibility
dashboard and package canary registry on `origin/main` commit
`db30ddac8776c0105ae8ebdefcd85541a6d11fc2`.

Commands:

```console
sed -n '1,35p' docs/architecture/runtime/node-compat-evidence/latest/status-summary.md
sed -n '1,18p' docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.md
node -e 'const fs=require("fs"); const data=JSON.parse(fs.readFileSync("tests/runtime/node/canary-registry.json","utf8")); const claims=data.claims||[]; const byPreset={}; const byFamily={}; for (const c of claims){byPreset[c.runtime_preset]=(byPreset[c.runtime_preset]||0)+1; byFamily[c.compat_family]=(byFamily[c.compat_family]||0)+1;} console.log(JSON.stringify({total:claims.length, byPreset, byFamily}, null, 2));'
find crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures -path '*test-process-features.js' -print
```

Lane baseline:

- Node24 `1002 / 5198`
- Node22 `1000 / 4748`
- Node26 `0 / 5578`
- package/framework canary claims `37`
- package/framework canary checks `101`
- diagnostic canary claims `11`
- required canary gaps `0`
- registry split `32` Application / `5` Tooling

| Lane | Role | Upstream | Vendored | Passed | Expected / known gap | Skipped / excluded | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| `node20` | legacy | `v20.20.2` | 1308 | 902 | 401 | 5 | 69.0% |
| `node22` | supported | `v22.22.3` | 4748 | 1000 | 3728 | 20 | 21.1% |
| `node24` | default | `v24.16.0` | 5198 | 1002 | 4149 | 47 | 19.3% |
| `node26` | current | `v26.2.0` | 5578 | 0 | 5529 | 49 | 0.0% |

Package/canary baseline:

| Metric | Count |
| --- | ---: |
| Package/framework canary claims | 37 |
| Package/framework canary checks | 101 |
| Diagnostic canary claims | 11 |
| Required canary gaps | 0 |
| Application registry claims | 32 |
| Tooling registry claims | 5 |

Current canary family split:

| `compat_family` | Claims |
| --- | ---: |
| `runtime-supplementary` | 1 |
| `networking` | 5 |
| `loader-context` | 6 |
| `convex` | 1 |
| `sdk` | 13 |
| `host-heavy` | 11 |

Canonical foundation slice denominator:

| Family | Slice |
| --- | --- |
| `core-semantics` | `assert-and-buffer-foundation` |
| `process-and-timing` | `process-foundation` |
| `streams-and-local-io` | `os-tty-readline-foundation` |
| `networking` | `dns-net-foundation` |
| `loader-context` | `module-and-async-foundation` |

`test-process-features.js` exists under all four vendored official fixture
lanes:

- `node20/test/parallel/test-process-features.js`
- `node22/test/parallel/test-process-features.js`
- `node24/test/parallel/test-process-features.js`
- `node26/test/parallel/test-process-features.js`

## Failure Grouping

Current broad gaps group into:

- **Default support denominator gap.** Node24 has 3825 fixtures classified as
  `Requires Unpromoted Node Surface`, so the current default support language is
  not strong enough for a well-supported default claim.
- **Supported LTS parity gap.** Node22 passes 1000/4748 and Node24 passes
  1002/5198. The percentages are close today, but both lanes need a new
  V8-isolate-required denominator and a much higher default-lane pass count.
- **Node26 paper-lane gap.** Node26 has vendored official fixtures and
  classifications, but the current generated dashboard has zero official
  fixture passes because no Node26 fixtures are promoted into the passed
  Rust-referenced evidence subset yet. NDS4 must prove the exact
  manifest/classification/harness changes that make Node26 runnable and
  positive.
- **Package evidence breadth gap.** The registry has 37 total claims and 32
  Application claims. NDS5 must reach at least 50 positive Application claims
  across at least 12 schema-controlled `compat_category` values.
- **Convex app realism gap.** The current registry has one real-app Convex
  canary claim. NDS6 must add at least 5 real Convex-compatible `"use node"` app
  suites.

## Focused Work

NDS0 scaffold work:

- Dedicated branch/worktree created.
- NDS hardening patch carried into the dedicated worktree without unrelated
  current-worktree changes.
- Baseline and control-plane proof files created.
- Verifier scaffold created to fail clearly on unimplemented completion gates.

No runtime compatibility fixes are made in NDS0.

## Broad Final Rerun

NDS0 scaffold verification:

```console
bash scripts/verify-node-default-runtime-support-hardening.sh
git diff --check
npm run docs:validate-refs:strict
```

Observed:

- `bash scripts/verify-node-default-runtime-support-hardening.sh`:
  `8 passed, 26 failed`.
- `git diff --check`: pass.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (241 working-tree Markdown files)`.

The verifier is expected to fail until later NDS rows land. The NDS0-specific
baseline and control-plane checks pass; the remaining failures are explicit
future-row gates.

## Evidence Links

- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.md`
- `docs/architecture/runtime/node-lts-compat/node-release-train.json`
- `tests/runtime/node/canary-registry.json`
- `docs/plans/archive/node-compat-cron-greening-plan.md`
- `docs/plans/node-default-runtime-support-hardening-plan.md`

## Residual Risks

- The draft PR is not open yet. NDS0 remains `in_progress` until the branch is
  pushed and the PR URL or developer-approved substitute is recorded.
- The main-visible Active Execution Pointer requires a narrow pointer update to
  `main` or an explicit fallback. Until that is recorded, the worktree/branch
  plus active goal are the operational pointer.
- The Node24 `2000` gate is intentionally not rebaselined here. NDS1 must prove
  feasibility or stop in the documented blocked state.
