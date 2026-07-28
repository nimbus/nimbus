# SWT0.2 Resource, Cold-Open, And Engine-Scale Checkpoint Baseline

Date: 2026-07-28, at `B_ref` `2a1853dab`.

## Peak RSS

`/usr/bin/time -l` over the accepted canonical CRUD run: maximum resident set
size **668,368,896 bytes (637.4 MiB)**. The cross-gate allows no increase
greater than 10% or 32 MiB, whichever is larger.

## Cold open

From the accepted layered run's connection table:

| Operation | Mean µs |
| --- | ---: |
| `Connection::open` only | 38.9 |
| Production-equivalent connection init on initialized DB | 459.2 |
| `SqliteTenantStore::open` + schema load | **424.5** |

The cold-open cross-gate (no regression >5% or 100 µs) anchors on 424.5 µs.

## Engine-scale WAL/checkpoint diagnostic (non-acceptance)

`wal-observation-raw.md`
(SHA-256 `1e1e03873176ac0471a29d892a3b2c55d3549cb66d89aa0b0119d6b043d34da0`),
canonical CRUD shape with `NIMBUS_CWB_WAL_CHECKPOINT_OBSERVATION=1`; sampled
aggregate WAL state per the documented attribution semantics; zero probe
errors on both rungs; probe share ≤0.368%.

| N | Foreground commits | Auto-checkpoint samples | Auto COMMIT upper bound | WAL high water | Auto threshold | Post-run PASSIVE busy/log/checkpointed |
| ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 9,000 | 35 | 15.434 ms | 1,007 frames | 1,000 | 0/385/385 |
| 256 | 1,966 | 75 | 320.331 ms | 1,087 frames | 1,000 | 0/773/773 |

This closes the D9 evidence gap: Engine-scale runs demonstrably cross the
automatic-checkpoint threshold during measurement (75 threshold samples in
one N=256 protocol run, WAL high water 1,087 frames), while the 768-mutation
layered fixture never does. Checkpoint tuning remains unauthorized (O6:
reassess only against this frozen baseline's counters); the observation mode
stays off in all canonical timed runs (D14).
