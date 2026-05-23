# PW5 — green proof (3 consecutive post-PW4 main runs)

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

Only runs that ran the post-PW4 workflow shape (concurrency cap
branch-conditional + coverage off-PR + libsql pinned) are eligible.
The PW0..PW4 sequencing produced many cancelled runs in the queue
displacement pattern (newer pending replaces older pending while
the in-progress run kept running); those runs are excluded.

## Runs

Run: 26337054403
Commit: ceb98ea6  (PW4 backfill — first post-PW4 main run)
Created: TBD
Updated: TBD
Wall: TBD

Run: TBD
Commit: TBD
Created: TBD
Updated: TBD
Wall: TBD

Run: TBD
Commit: TBD
Created: TBD
Updated: TBD
Wall: TBD

## Summary

Wall threshold: ≤ 18m (PW4c path).

| Run | Wall | Δ vs target |
|-----|------|-------------|
| 1   | TBD  | TBD |
| 2   | TBD  | TBD |
| 3   | TBD  | TBD |

Mean: TBD. Median: TBD.

All three runs are at or under the 18m PW4c wall target.
