# CW0 — baseline

Pre-CW1 snapshot of PR CI on `main` after the CA wave closed.

## Reference commit + run

- Commit: `32951ee7` — DU7 proof: fresh-build embed verification on
  main post-CA (a docs-only commit; changes are 4 PNG screenshots +
  one Markdown file, so its CI run is as close as we can get to
  "pure infrastructure cost on the post-CA control state").
- CI workflow run: `26324236849`
- Wall: **23m34s** (05:08:59Z → 05:32:33Z)

## Per-job timing (post-CA, this commit)

Sorted by duration. Critical-path-gated annotations: a job gated on
`warm-sccache` cannot start until warm-sccache ends (t≈10:52m from
workflow start).

| Job                                  | Duration | Gated on warm-sccache? | Critical path |
|--------------------------------------|----------|------------------------|---------------|
| Rust Workspace Tests                 | **15.7m** | no                    | t=0:00→15:45   |
| Server Verification Harness          | **12.7m** | **yes**                | t=10:54→23:33 ← workflow wall |
| Rust Clippy                          | 11.7m    | no                     | t=0:00→11:43   |
| Engine Verification Harness          | 11.0m    | yes                    | t=10:54→21:55  |
| Warm sccache (leader)                | 10.2m    | n/a                    | t=0:41→10:52   |
| Rust Runtime Tests                   | 9.9m     | no                     | t=0:02→9:57    |
| Storage Verification Harness         | 8.3m     | yes                    | t=10:54→19:15  |
| External Provider Integration Tests  | **14.6m** | no                    | t=0:02→14:35   |
| Rust Dependency Audit                | 5.2m     | no                     | t=0:02→5:14    |
| Coverage shard (engine)              | 4.5m     | yes                    | t=10:54→15:24  |
| Coverage shard (server)              | 4.3m     | yes                    | t=10:54→15:09  |
| Coverage reducer                     | 3.8m     | yes                    | t=15:26→19:10  |
| Coverage shard (rest)                | 3.0m     | yes                    | t=10:54→13:57  |
| JavaScript Build and Test            | 1.5m     | no                     | t=0:02→1:30    |
| Runtime Verification Harness         | 1.4m     | yes                    | t=10:54→12:21  |
| UI Artifacts                         | 0.6m     | n/a (leader)           | t=0:03→0:39    |
| Rust Format                          | 0.2m     | no                     | t=0:02→0:12    |
| Proof Helper Checks                  | 0.2m     | no                     | t=0:02→0:17    |
| Rust Gate Summary                    | 0.0m     | post-aggregator        |                |

## Critical path

```
workflow-start (t=0:00)
  → ui-artifacts (0.6m)
  → warm-sccache (10.2m)                    [ends t=10:52]
  → Server Verification Harness (12.7m)     [ends t=23:33]
  → workflow-end (t=23:34)
```

= **22.9m of critical-path serialized work**, close to the 23.6m wall.

## Lateral poles (not gated on warm-sccache)

The longest jobs that run concurrently with the critical path and so
do not extend the wall directly, but would gate it if the critical
path shrinks:

- **Rust Workspace Tests** at 15.7m — single un-sharded `cargo nextest
  run --workspace` job. If the critical path drops to ~12-15m, this
  becomes the wall.
- **External Provider Integration Tests** at 14.6m — postgres + mysql
  + libsql, serialized within-provider via `serial_test::serial(<provider>)`,
  but the three providers run sequentially in a single job rather than
  as a matrix.
- **Rust Clippy** at 11.7m — clippy runs its own `cargo check` pass.
  sccache helps but doesn't eliminate.

## CA delta (Coverage is no longer the pole)

Pre-CA baseline (CA0 measured `33m57s` on `f99f7d6c`, CM4 hotfix):

| Job                                  | Pre-CA  | Post-CA (CW0) | Delta |
|--------------------------------------|---------|---------------|-------|
| Coverage path                        | 24m27s  | 4.5m max shard + 3.8m reducer = 8.3m | **−16.2m** |
| Rust Workspace Tests                 | 14m16s  | 15.7m         | +1.4m |
| Server Verification Harness          | 10m59s  | 12.7m         | +1.7m |
| External Provider Integration Tests  | 10m12s  | 14.6m         | +4.4m |
| Engine Verification Harness          | 9m39s   | 11.0m         | +1.4m |
| Storage / Runtime Verification Harness | ≤ 8m07s | 8.3m / 1.4m  | ~flat |
| Rust Clippy                          | 6m39s   | 11.7m         | +5.1m |
| Warm sccache (leader)                | 5m50s   | 10.2m         | +4.4m |
| Workflow wall                        | 33m57s  | 23m34s        | **−10m23s** |

The CA win was real but smaller than the headline projection (33m →
~17m claimed in CA's `Why this plan exists`). Coverage path collapsed
as designed; the workflow wall came down ~10m. Several other jobs
appear ~1-5m slower than the pre-CA snapshot, partly noise (one-commit
samples) but partly real: warm-sccache appears to be doing more (CC9
shared-key v1→v2 rotated all caches once; subsequent runs are
re-populating the v2 caches), and Rust Clippy grew because the
clippy-lint set expanded mid-wave.

## Why CW exists

The CA wave's measured win at the workflow-wall level was ~10m on a
heavy-touch commit; a steady-state estimate is ~6-8m of true CA wall
delta. To go further, CW has to attack the post-CA poles directly.
The plan's targets:

- **CW1** (verification-harness sharding): Path A pole 22.9m →
  ~15.2m. Each surface gets shard count tuned to its current duration
  (server: 3, engine: 2, storage: 2, runtime: 1).
- **CW2** (workspace-tests sharding via nextest --partition): lateral
  pole 15.7m → ~6m max shard.
- **CW3** (provider matrix split): lateral pole 14.6m → ~7m
  (postgres-dominated; the remaining headroom is intra-postgres
  parallelism, deferred).
- **CW4** (warm-sccache shrink): if either lane lands, path A pole
  drops further.
- Wall target post-CW1+CW2+CW3: ~12-15m on a comparable doc-only
  commit. Floor without infrastructure changes: ~12m.

## Repro commands

Replicate this snapshot:

```bash
gh run view 26324236849 --json jobs,createdAt,updatedAt,workflowName \
  | jq '{wall_start: .createdAt, wall_end: .updatedAt,
         jobs: [.jobs[] | {name, started: .startedAt,
                            completed: .completedAt, conclusion}]}'
```

Or list the last N successful CI runs on `main` and dump wall
durations:

```bash
gh run list --branch main --workflow CI --status success --limit 10 \
  --json headSha,createdAt,updatedAt
```

## Notes

- Single-sample timings are noisy; CW1..CW3 should land separately
  and validate per-stage rather than measuring a CW0→CW5 jump in one
  go.
- The doc-only commit was selected deliberately. CW0's purpose is to
  measure infrastructure cost, not test-suite cost. A code-touching
  commit would conflate workflow-shape findings with test-output
  findings.
- Caches are warm on this run (sccache populated by predecessors).
  CW1+ proof commits will similarly benchmark on warm caches; if a
  cache-cold pass becomes load-bearing, capture both in proof.
