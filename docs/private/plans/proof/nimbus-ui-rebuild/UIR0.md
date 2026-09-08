# UIR0 Baseline

Date: 2026-09-08
Baseline: main @ 67a7f3ffa
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild`

## Verifier (fail-before)

`bash docs/private/plans/proof/nimbus-ui-rebuild/verify.sh` → `verify: 20 failing`, exit 1.
Green rows on the baseline: no "Coming soon" string in `src` (the nav dead-ends
render from a different label), and specs for six of the seven route groups.
`routes/developer/settings` has no spec on the baseline (UIR13 adds one).

## Baseline checks (`packages/nimbus-ui`)

| Command | Result |
| --- | --- |
| `npm run test` | 98 files, 851 tests passed, 7.31s |
| `tsc -p tsconfig.json --noEmit` | exit 0 |
| `biome check src` | 1 pre-existing format error: `src/test/msw.spec.ts` (237 files checked) |
| `npm run build`, `npm run test:e2e:smoke` | UNVERIFIED at baseline (run at UIR1) |

## Baseline screenshots

`baseline/s17.jpg` … `baseline/s30.jpg` from the 2026-09-08 design review:
s17 Overview (tenant present), s18 Tenants, s19 Compute, s20 function detail,
s21–s23 runner and runs, s24 Logs empty, s25 Storage tables, s26 Overview
populated, s27–s28 light Overview and Storage, s29 documents, s30 runs.

## Production behavior

None changed.
