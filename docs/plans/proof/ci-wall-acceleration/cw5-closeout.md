# CW5 — closeout

CW5 closes the CI Wall Acceleration wave. Every ledger row is `done`,
the plan is moved to `docs/plans/archive/`, routing in
`docs/plans/README.md` + `AGENTS.md` (a.k.a. `CLAUDE.md`) points at
the archived path, and the canonical contract is consolidated in
`docs/operating/ci-modernization.md`'s "PR critical-path acceleration"
section.

## What landed

| CW  | Commit     | Subject |
|-----|------------|---------|
| CW0 | `31e88d01` | scaffold CI Wall Acceleration plan + verifier + baseline proof |
| CW1 | `523834cb` | shard verification-harness corpus across N shards per surface |
| CW2 | `bacab0e1` | shard Rust Workspace Tests via nextest --partition |
| CW3 | `74e0ef8c` | split External Provider Integration Tests by provider |
| CW4 | `b58024e0` | warm-sccache --tests drop (lane (a) landed; lane (b) deferred) |
| CW5 | _this commit_ | closeout — archive plan, promote contract, update routing |

## Wall-time targets vs landed shape

The plan's projected post-CW4 wall floor (without infra changes) was
~12-15m, gated by `warm-sccache + 1 server-harness-shard` or
`cold-build cost of the longest test binary`. The structural changes
landed match those projections:

| Pole | CW0 baseline | Landed shape | Notes |
|------|--------------|--------------|-------|
| Server Verification Harness | 12.7m | 4 shards (`1/4`..`4/4`) ~3.5m each | CW1 |
| Rust Workspace Tests | 15.7m | 3 partitions (`hash:N/3`) ~5-6m each | CW2 |
| External Provider Integration Tests | 14.6m | 3-way matrix (postgres / mysql / libsql) ~6-7m each | CW3 |
| warm-sccache | 10.2m | `cargo check --workspace` (no `--tests`) | CW4 |
| Engine Verification Harness | 11.0m | 2 shards (`1/2`, `2/2`) ~6m each | CW1 |
| Storage Verification Harness | 8.3m | single-shard (below wall pole) | CW1 |
| Runtime Verification Harness | 1.4m | single-shard (already fast) | CW1 |

The actual wall delta vs CW0 baseline will be measured by the next
post-CW5 CI run on `main`. The structural argument: every wall pole
above 10m is now sharded across runners, so the critical path
collapses to `warm-sccache + 1 server-harness-shard ≈ 13.7m` or
`max(workspace shard, provider shard, server-harness shard) ≈ 6-7m`
running parallel to warm-sccache (10.2m, post lane-a possibly less).

## What was deferred

- **CW4 lane (b)**: per-target Swatinem cache layer. Deferred because
  `Swatinem/rust-cache@v2` already caches `target/`; a bespoke
  additional layer requires CI-run measurement to justify the added
  complexity. Re-open if `warm-sccache` returns as the wall pole and
  lane (a)'s gains plateau.
- **Rust Clippy** (11.7m on CW0): not sharded in this wave. `cargo
  clippy --workspace` runs serially and there is no equivalent of
  `nextest --partition` for the lint pass. Listed in CW2's deferred
  scope and the CW4 proof as a candidate for a future wave if it
  becomes the wall pole post-CW1-4.
- **Rust Runtime Tests** (9.9m on CW0): not sharded in this wave. The
  runtime crate's V8 state-isolation requirements would need careful
  per-shard cleanup; the duration is already below the warm-sccache
  pole so the wall delta from sharding is small.
- **CI run measurement**: the CW0 baseline lives in
  `cw0-baseline.md`. A post-CW5 wall measurement on the next green
  `main` run will confirm the projection above. The verifier's
  condition 10 already gates on the latest `main` run being green
  (`status=completed conclusion=success`).

## Routing changes

- `docs/plans/README.md`: removed `docs/plans/ci-wall-acceleration-plan.md`
  from the "Active execution plans" section; added a
  `docs/plans/archive/ci-wall-acceleration-plan.md` entry under
  "Current Reference Baselines" summarizing the CW0..CW5 contracts.
- `AGENTS.md` (`CLAUDE.md` is a symlink): updated the routing block
  to point at the archived path and the canonical contract section
  in `docs/operating/ci-modernization.md`. The previous "the active
  plan is..." framing is replaced with the standard "canonical
  contract first, then completed baseline" shape used by prior closed
  waves (CC, CM, CA).
- `docs/operating/ci-modernization.md`: gains a "PR critical-path
  acceleration" section (added in CW4's commit) synthesizing the
  CW1..CW4 contracts in one place. The section sits between the
  "Coverage and release acceleration" section and the
  "Deferred follow-up: Windows release pole" section.
- The verifier's `plan_file()` helper (`scripts/verify-ci-wall-acceleration.sh`)
  was already shaped to accept both `docs/plans/ci-wall-acceleration-plan.md`
  and `docs/plans/archive/ci-wall-acceleration-plan.md`, so the
  archival move does not require a verifier change.

## Verification

`bash scripts/verify-ci-wall-acceleration.sh` exits with `10 passed, 0
failed` once the post-CW5 CI run on `main` finishes green. The 10
conditions:

1. Plan exists (active or archived path) — archived path now used.
2. `CLAUDE.md` references `ci-wall-acceleration-plan` — preserved via
   the routing rewrite.
3. CW0 baseline proof exists at
   `docs/plans/proof/ci-wall-acceleration/cw0-baseline.md`.
4. Harness script accepts a third positional shard arg + corpus tests
   honor `NIMBUS_HARNESS_SHARD`.
5. harness job matrix includes per-surface shard expansion.
6. `rust-workspace-tests` uses `nextest --partition` with matrix.
7. `external-provider-tests` job has provider matrix with ≥ 3 entries.
8. Warm-sccache lane documented in `ci-modernization.md` and landed.
9. Every ledger row is marked `done`.
10. Latest CI run on main is green.

Conditions 1-9 pass deterministically against the working tree.
Condition 10 turns green once the CW5 push completes its CI run.

## What's next

- Watch the next green `main` CI run to measure wall delta vs CW0
  baseline. Capture the result in a follow-up note under
  `docs/plans/proof/ci-wall-acceleration/` if useful.
- If wall plateaus above the projected ~12-15m floor, consider a
  follow-on plan attacking the next pole (clippy or runtime tests).
- The CW0 baseline file (`cw0-baseline.md`) remains the canonical
  comparison point for any future PR-wall investigation.
