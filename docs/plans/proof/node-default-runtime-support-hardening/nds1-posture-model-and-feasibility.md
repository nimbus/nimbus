# NDS1 Posture Model And Feasibility Proof

status: done
date: 2026-06-01
branch: codex/node-default-runtime-support-hardening
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening
pr: https://github.com/nimbus/nimbus/pull/10
verifier: scripts/verify-node-default-runtime-support-hardening.sh

## Row And Status

NDS1 is done. The default-support posture is generated from the checked-in
official fixture evidence and lane classification catalogs, with a
schema-controlled denominator overlay that removes `Requires Unpromoted Node
Surface` as a completion bucket without hiding the original source count.

## Broad Pre-Run

Commands:

```console
sed -n '1,35p' docs/architecture/runtime/node-compat-evidence/latest/status-summary.md
python3 scripts/runtime/node/default_support_posture.py
python3 scripts/runtime/node/default_support_posture.py --check
```

Baseline official fixture posture before NDS1:

| Lane | Passed | Vendored | Source `Requires Unpromoted Node Surface` |
| --- | ---: | ---: | ---: |
| `node22` | 1000 | 4748 | 3417 |
| `node24` | 1002 | 5198 | 3825 |
| `node26` | 0 | 5578 | 5233 |

Generated NDS posture after NDS1:

| Lane | Required gaps | Optional gaps | Diagnostic gaps | Harness-only gaps | Upstream/platform gaps | Remaining unpromoted |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `node22` | 1290 | 366 | 1670 | 396 | 26 | 0 |
| `node24` | 1446 | 401 | 1750 | 576 | 23 | 0 |
| `node26` | 2085 | 595 | 2245 | 630 | 23 | 0 |

Artifacts:

- `scripts/runtime/node/default_support_posture.py`
- `tests/runtime/node/schemas/node-default-support-posture.schema.json`
- `docs/architecture/runtime/node-default-support-posture.json`
- `docs/architecture/runtime/node-default-support-posture.md`

## Failure Grouping

The source classification catalogs still expose the old broad gap label so the
baseline remains auditable. NDS1 groups those gaps into a separate
default-support denominator:

- `v8_isolate_required`: public Node API behavior that should be green for the
  V8-isolate Application contract.
- `v8_isolate_optional`: visible, promotable gaps that are not required for the
  default Application contract in this wave.
- `diagnostic_only_non_isolate`: host-owned process/socket/native/signal/worker
  behavior that must fail closed or route to a host-capable backend.
- `test_harness_only`: upstream harness topology, pummel/stress, WPT, pseudo
  terminal, or support files.
- `upstream_or_platform_boundary`: upstream, version-specific, or platform-only
  boundary.

Every generated entry carries:

- `support_denominator`
- `reason_code`
- `reason`
- `evidence_path`
- `docs_cross_check`
- `shim_classification`

## Focused Work

Implemented a generator instead of hand-maintaining posture files:

- Reads `docs/architecture/runtime/node-compat-evidence/latest/status-summary.json`.
- Reads expanded lane classification entries from the published status summary.
- Maps every classified gap into exactly one NDS denominator.
- Preserves source counts, including the original unpromoted counts.
- Emits a Node24 feasibility estimate.
- Provides `--check` mode so stale generated files fail verification.

Node24 feasibility estimate:

| Metric | Count |
| --- | ---: |
| Current passed | 1002 |
| Required gap count | 1446 |
| Optional promotable gap count | 401 |
| Estimated reachable pass ceiling | 2849 |
| Target pass count | 2000 |
| Target reachable in this plan | true |

This estimate says the `2000` target is reachable without lowering the target.
It is not a completion claim. NDS3 may still re-enter the blocked path if
implementation proves the estimate wrong.

## Broad Final Rerun

Commands:

```console
python3 scripts/runtime/node/default_support_posture.py --check
bash scripts/verify-node-default-runtime-support-hardening.sh
git diff --check
npm run docs:validate-refs:strict
```

Observed:

- `python3 scripts/runtime/node/default_support_posture.py --check`:
  `node default support posture: pass`.
- `bash scripts/verify-node-default-runtime-support-hardening.sh`:
  `13 passed, 21 failed`.
- `git diff --check`: pass.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (242 working-tree Markdown files)`.

The remaining verifier failures are future-row gates for NDS2-NDS10 and final
closeout.

## Evidence Links

- `docs/architecture/runtime/node-default-support-posture.json`
- `docs/architecture/runtime/node-default-support-posture.md`
- `tests/runtime/node/schemas/node-default-support-posture.schema.json`
- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.json`
- `tests/runtime/node/classifications/node22.json`
- `tests/runtime/node/classifications/node24.json`
- `tests/runtime/node/classifications/node26.json`

## Residual Risks

- **NDS3 accounting amendment (2026-06-01).** During NDS3, the status generator
  was corrected to count only matching-lane non-ignored Rust tests that execute
  official Node compatibility fixtures. The historical `1002 / 5198` Node24
  number in this proof is now treated as a source-topology baseline, not an
  execution pass numerator. The corrected execution-only posture before the
  first NDS3 broad promotion was Node22 `1024 / 4773`, Node24 `160 / 5198`, and
  Node26 `0 / 5578`. After promoting the Node24 `core-semantics` broad group,
  the generated posture reports Node24 `276 / 5198`, required gaps `1770`,
  optional promotable gaps `434`, and an estimated reachable ceiling of `2480`.
  The `2000` target remains reachable on paper but still requires substantial
  NDS3 implementation work.
- The denominator overlay is intentionally conservative but heuristic. NDS3 must
  prove it by greening required clusters or moving fixtures to a stricter
  blocked/diagnostic path with proof.
- The Node24 `2000` feasibility ceiling is an estimate from current catalogs. It
  does not guarantee every required/optional fixture is cheap to fix.
- Source classification catalogs still contain `Requires Unpromoted Node
  Surface`; the NDS posture removes it from the default-support denominator
  while preserving the source count for auditability.
