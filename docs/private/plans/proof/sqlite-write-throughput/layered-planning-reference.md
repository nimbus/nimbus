# Layered SQLite Planning Reference

Base: `e47b64eacc3d54dc5bfe7d51727306a81cfacb28`

Date: 2026-07-27

Workload: 256 disjoint phased CRUD units / 768 logical mutations; captured
batch distribution `[5, 251, 90, 256, 20, 146]`; 12 measured samples; 60
fresh-database repetitions/sample; WAL + `synchronous=FULL`; repository
rusqlite/SQLCipher build.

Verdict: **clean planning reference, not an acceptance or A/B baseline**.
Every lane met the ≤10% CV policy, but the exact executable that produced
this report was overwritten before its SHA-256 was captured. The report
itself has SHA-256
`6b4f69edf5b040822195deb699826bdbfcdf9e0a15e59f2033a16aba45b28574`.
The values remain useful for ranking mechanisms, while SWT0 must establish
the cryptographically bound same-session baseline before any candidate can be
accepted.

## Result

| Lane | Logical mut/s | 95% CI | Median | CV | SQL stmt/s | Row changes/s | Tx/s | Sync commits/s | Mean |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Raw one-row CRUD | 305,895 | 291,708–320,082 | 308,703 | 7.3% | 310,675 | 305,895 | 2,389.8 | 2,389.8 | 2.523 ms |
| Current-loop SQL, resident connection | 50,358 | 49,688–51,029 | 50,725 | 2.1% | 422,866 | 151,862 | 786.8 | 786.8 | 15.257 ms |
| Guarded prepared/hoisted SQL | 151,485 | 149,141–153,830 | 151,379 | 2.4% | 670,836 | 456,823 | 2,367.0 | 2,367.0 | 5.073 ms |
| Nimbus-shaped SQL lower bound | 171,088 | 168,132–174,044 | 171,857 | 2.7% | 523,957 | 515,937 | 2,673.3 | 2,673.3 | 4.492 ms |
| Production storage append+apply | 38,810 | 38,319–39,301 | 39,018 | 2.0% | 337,567 | 117,138 | 606.4 | 606.4 | 19.796 ms |

The guarded lane retains live preimage reads and resource-binding cleanup. The
lower-bound lane omits those queries only to price their maximum lower-layer
cost; it is not an authorized production fast path.

All per-second columns above use the mean logical-fixture throughput as their
single estimator. The original planning report derived transaction rates from
mean elapsed separately; those five displayed transaction/s and sync/s cells
were corrected arithmetically here. Logical throughput, confidence intervals,
statement rates, row rates, elapsed times, and raw samples are unchanged.

## Bytes and checkpoint state

| Lane | DB bytes | WAL bytes | Page | WAL frames | Passive-checkpointed | Autocheckpoint |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Raw | 4,096 | 700,432 | 4,096 | 170 | 170 | 1,000 |
| Current-loop resident | 4,096 | 1,404,952 | 4,096 | 341 | 341 | 1,000 |
| Guarded prepared/hoisted | 4,096 | 1,404,952 | 4,096 | 341 | 341 | 1,000 |
| Nimbus-shaped lower bound | 4,096 | 1,404,952 | 4,096 | 341 | 341 | 1,000 |
| Production storage | 4,096 | 1,396,712 | 4,096 | 339 | 339 | 1,000 |

WAL byte size was captured before the explicit passive-checkpoint probe. The
probe reported all listed frames checkpointable. No lane reached the default
1,000-page automatic-checkpoint threshold.

## Raw measured-round samples

- **Raw:** 312458.595, 307104.628, 280909.666, 306045.019, 276359.092,
  292665.658, 275440.548, 354402.945, 318102.518, 310300.586,
  317402.827, 319549.158
- **Current-loop resident:** 50196.675, 50483.278, 51244.428, 51022.214,
  50814.618, 49073.232, 50634.487, 50857.805, 49524.721, 51347.045,
  51257.489, 47844.710
- **Guarded prepared/hoisted:** 155381.860, 154231.297, 142649.124,
  154590.493, 155171.283, 149961.658, 154421.589, 151965.402,
  148347.547, 149581.332, 150792.209, 150731.804
- **Nimbus-shaped lower bound:** 159919.115, 173172.347, 176573.429,
  167232.416, 168275.215, 177712.445, 169712.241, 172477.655,
  170548.493, 171235.815, 173131.518, 173065.139
- **Production storage:** 39012.619, 38728.408, 38781.631, 39023.395,
  39611.022, 39620.449, 39480.393, 39207.482, 39381.677, 37596.798,
  37756.836, 37519.804

## CPU-only serialization and connection cost

| Measurement | Result |
| --- | ---: |
| Record MessagePack + current fields/typed JSON | 669,480 logical mut/s |
| Same serialization per fixture | 1.147 ms |
| `Connection::open` only | 45.2 µs |
| Open + production-equivalent initialization | 494.1 µs |
| `SqliteTenantStore::open` + schema load | 494.2 µs |

## Statement count model

| Lane | Statements/fixture | Basis |
| --- | ---: | --- |
| Raw | 780 | 768 DML + 12 transaction-control |
| Current-loop resident | 6,449 | Exact benchmark-issued statements |
| Guarded prepared/hoisted | 3,401 | Exact benchmark-issued statements |
| Nimbus-shaped lower bound | 2,352 | Exact benchmark-issued statements |
| Production storage | 6,680 | Source count: 6,452 workload + 228 per-open initialization |

The count excludes the busy-handler C API and SQLite-internal statements.

## Command

```bash
timeout 600 env \
  NIMBUS_SWO_ROUNDS=12 \
  NIMBUS_SWO_REPETITIONS_PER_SAMPLE=60 \
  NIMBUS_SWO_OUT=/tmp/sqlite-write-overhead-layered-final.md \
  /Users/jack/src/github.com/nimbus/nimbus/target/release/deps/sqlite_write_overhead-afcd9628c6b34dc8
```
