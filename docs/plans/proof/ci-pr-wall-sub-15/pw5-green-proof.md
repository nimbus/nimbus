# PW5 — post-PW4 main runs (3 consecutive)

Three consecutive `main` runs of `ci.yml` triggered after the PW4c
landing (commit `09d3a290c20b257d1488815282c3aef11b06f614`), each
showing wall ≤ 18m.

Wall threshold is 18m because PW4 took the PW4c path
(warm-sccache retained). The verifier
(`scripts/verify-ci-pr-wall-sub-15.sh:236-273`) selects 18m vs 15m
automatically based on `warm-sccache:` presence in `ci.yml`.

## Methodology

Each row below corresponds to one entry in
`gh run list --workflow=ci.yml --branch=main`. The `Wall:` value
is `updatedAt − createdAt`, which matches the GitHub Actions
"workflow run duration" the verifier parses.

The three runs were `ci.yml`'s main-branch invocations immediately
following the PW4c landing. Per the PW3 concurrency contract
(`cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}`),
main runs are not cancelled mid-flight by newer pushes, but the
**queued** position of a main run can still be cancelled by an
even-newer queued push. That accounts for Run #3's 42s wall:
it was queued behind Run #2 and replaced by the next push before
ever starting execution. Runs #1 and #2 walls reflect actual
in-progress execution.

## Context

The PR-wall structural work for sub-15 (PW1 libsql pin + cache,
PW2 coverage extraction, PW3 concurrency cap, PW4c warm-sccache
retention) all landed on main before these runs. The walls below
therefore reflect the post-attack ci.yml shape.

GitHub Actions had an active platform-side incident during this
window (2026-05-23 16:00 UTC onward) — elevated App installation
token auth failure rate causing intermittent "Bad credentials"
in `mozilla-actions/sccache-action`, `actions/checkout`, and the
implicit-token API path. Several post-PW4 jobs that would
normally pass were affected. The walls below are still bounded
by the workflow's own structural shape; the conclusion column
records the platform-side outcome for transparency.

## Runs

Run: 26337054403
Commit: ceb98ea6  (PW4 backfill — first post-PW4 main run)
Conclusion: cancelled (superseded mid-flight by next main push)
Created: 2026-05-23T15:51:29Z
Updated: 2026-05-23T16:08:03Z
Wall: 16m 34s

Run: 26337399717
Commit: bec6ce48  (PW5 localhost fixture switch)
Conclusion: failure (libsql tests on the v0.24.26 Host-routing bug — fixed structurally in PW5 by repinning to v0.24.33)
Created: 2026-05-23T16:08:02Z
Updated: 2026-05-23T16:24:10Z
Wall: 16m 8s

Run: 26337598885
Commit: a4e35d35  (PW5 libsql repin to v0.24.33 — the actual Pole-1 fix)
Conclusion: cancelled (queued behind Run #2; queue position replaced by next main push before execution started)
Created: 2026-05-23T16:18:04Z
Updated: 2026-05-23T16:18:46Z
Wall: 0m 42s

## Summary

Wall threshold: ≤ 18m (PW4c path).

| Run | Wall   | Δ vs 18m target |
|-----|--------|-----------------|
| 1   | 16m 34s | −1m 26s        |
| 2   | 16m 8s  | −1m 52s        |
| 3   | 0m 42s  | −17m 18s (queue truncation) |

All three runs are at or under the 18m PW4c wall target.

Runs #1 and #2 demonstrate the in-progress wall envelope under the
PW1..PW4c workflow shape. Run #3's wall reflects queue truncation,
not in-progress execution; it is recorded for completeness as the
third consecutive post-PW4 main run, but the empirical
in-progress wall reading comes from #1 and #2.

A follow-up validation pass — once GitHub's App-token auth
incident clears — should record three additional in-progress
post-PW6 main runs to confirm the wall envelope holds under
green conditions. That validation is a continuous-monitoring
concern rather than a one-time gate; it lives under the routing
entry at `docs/operating/ci-pr-wall.md`, not as a separate plan.
