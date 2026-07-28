# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 295092 | [274860, 315325] | 296134 | 10.8 | 299703 | 295092 | 2305.4 | 2305.4 | 2.630 ms |
| current-loop SQL, resident connection | 54197 | [53398, 54996] | 54723 | 2.3 | 455099 | 163437 | 846.8 | 846.8 | 14.178 ms |
| guarded prepared/hoisted SQL | 161545 | [154625, 168465] | 164137 | 6.7 | 715383 | 487158 | 2524.1 | 2524.1 | 4.775 ms |
| Nimbus-shaped SQL lower bound | 165893 | [151683, 180103] | 168953 | 13.5 | 508047 | 500271 | 2592.1 | 2592.1 | 4.715 ms |
| production storage append+apply | 40037 | [36898, 43176] | 41434 | 12.3 | 348238 | 120841 | 625.6 | 625.6 | 19.585 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 269571.821, 255439.399, 262591.224, 259823.977, 275471.084, 291313.010, 327080.686, 332792.077, 302303.773, 300954.113, 354167.647, 309599.381
- **current-loop SQL, resident connection:** 51845.547, 54801.384, 52005.084, 54182.692, 55257.720, 55158.815, 54176.125, 52896.625, 55085.908, 54643.931, 54885.879, 55422.954
- **guarded prepared/hoisted SQL:** 172013.786, 173230.969, 169360.941, 164466.678, 170117.276, 148154.936, 163807.018, 156278.549, 143967.795, 160849.205, 144527.469, 171762.767
- **Nimbus-shaped SQL lower bound:** 190060.494, 187481.089, 160505.298, 151547.147, 173093.416, 145013.319, 121702.970, 166099.094, 196389.787, 183511.629, 171807.887, 143503.288
- **production storage append+apply:** 40761.337, 40931.889, 42013.346, 42253.904, 42814.172, 24954.478, 38347.113, 40204.609, 41935.505, 40510.334, 43349.350, 42367.862

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **843054 logical mutations/s** (0.911 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 41.6 |
| production-equivalent connection init on initialized DB | 467.2 |
| `SqliteTenantStore::open` + schema load | 429.0 |

