# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 416839 | [408502, 425177] | 417992 | 3.1 | 423353 | 416839 | 3256.6 | 3256.6 | 1.844 ms |
| current-loop SQL, resident connection | 54783 | [54168, 55399] | 54973 | 1.8 | 460024 | 165206 | 856.0 | 856.0 | 14.023 ms |
| guarded prepared/hoisted SQL | 172286 | [171012, 173560] | 172573 | 1.2 | 762950 | 519550 | 2692.0 | 2692.0 | 4.458 ms |
| Nimbus-shaped SQL lower bound | 178769 | [165522, 192016] | 188472 | 11.7 | 547481 | 539101 | 2793.3 | 2793.3 | 4.364 ms |
| production storage append+apply | 42576 | [41518, 43634] | 43045 | 3.9 | 370325 | 128505 | 665.3 | 665.3 | 18.065 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 416142.049, 422147.619, 398167.723, 413611.721, 419842.114, 408802.972, 392065.116, 434626.780, 432038.141, 422577.494, 431114.713, 410937.362
- **current-loop SQL, resident connection:** 55074.197, 54794.621, 54957.768, 55533.286, 55768.915, 54901.990, 54988.551, 55343.723, 52115.505, 53793.766, 54902.854, 55226.594
- **guarded prepared/hoisted SQL:** 173869.297, 172026.443, 170975.636, 168340.398, 174243.303, 174203.205, 173414.647, 174815.486, 172979.897, 172165.892, 170248.530, 170151.228
- **Nimbus-shaped SQL lower bound:** 191062.828, 190983.806, 126855.002, 192187.149, 189851.088, 146036.680, 187093.380, 183464.415, 179979.906, 191384.284, 190952.680, 175380.648
- **production storage append+apply:** 43224.623, 42375.674, 42571.724, 40514.483, 42866.214, 40844.655, 38959.368, 44027.330, 44203.312, 43276.594, 44319.955, 43731.796

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **841361 logical mutations/s** (0.913 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 38.7 |
| production-equivalent connection init on initialized DB | 442.9 |
| `SqliteTenantStore::open` + schema load | 414.3 |

