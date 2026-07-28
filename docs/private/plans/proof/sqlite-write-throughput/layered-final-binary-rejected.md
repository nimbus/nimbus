# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 287831 | [270026, 305636] | 294248 | 9.7 | 292328 | 287831 | 2248.7 | 2248.7 | 2.694 ms |
| current-loop SQL, resident connection | 45426 | [44291, 46560] | 45936 | 3.9 | 381445 | 136987 | 709.8 | 709.8 | 16.931 ms |
| guarded prepared/hoisted SQL | 125526 | [117920, 133132] | 130714 | 9.5 | 555877 | 378539 | 1961.3 | 1961.3 | 6.183 ms |
| Nimbus-shaped SQL lower bound | 139807 | [136397, 143216] | 139463 | 3.8 | 428157 | 421604 | 2184.5 | 2184.5 | 5.501 ms |
| production storage append+apply | 31719 | [29645, 33792] | 32599 | 10.3 | 275885 | 95734 | 495.6 | 495.6 | 24.489 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 326648.566, 291157.919, 264269.742, 283580.571, 264163.131, 227419.896, 265991.677, 308356.252, 297337.730, 301649.733, 308636.873, 314756.517
- **current-loop SQL, resident connection:** 48973.022, 46246.292, 45936.164, 45936.615, 46016.780, 46463.941, 43009.212, 46389.937, 42048.283, 44725.187, 44887.821, 44474.763
- **guarded prepared/hoisted SQL:** 130904.955, 91774.493, 132734.235, 133801.036, 125767.510, 130522.606, 132651.970, 127426.902, 131404.439, 133128.416, 116415.977, 119777.302
- **Nimbus-shaped SQL lower bound:** 147487.366, 143344.943, 131002.474, 136029.972, 138668.833, 138235.506, 136298.848, 142724.806, 140257.214, 133675.162, 140639.716, 149313.479
- **production storage append+apply:** 35027.189, 31648.970, 34406.499, 32744.962, 34064.607, 34066.203, 32452.441, 33817.007, 31229.521, 29335.924, 28002.059, 23826.768

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **670844 logical mutations/s** (1.145 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 44.2 |
| production-equivalent connection init on initialized DB | 612.7 |
| `SqliteTenantStore::open` + schema load | 657.4 |

