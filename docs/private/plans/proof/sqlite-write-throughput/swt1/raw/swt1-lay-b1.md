# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 391642 | [374588, 408695] | 387707 | 6.9 | 397761 | 391642 | 3059.7 | 3059.7 | 1.969 ms |
| current-loop SQL, resident connection | 54303 | [53820, 54787] | 54290 | 1.4 | 455990 | 163758 | 848.5 | 848.5 | 14.145 ms |
| guarded prepared/hoisted SQL | 163677 | [161892, 165463] | 164104 | 1.7 | 724826 | 493589 | 2557.5 | 2557.5 | 4.693 ms |
| Nimbus-shaped SQL lower bound | 181386 | [178389, 184384] | 179789 | 2.6 | 555495 | 546993 | 2834.2 | 2834.2 | 4.237 ms |
| production storage append+apply | 43333 | [43018, 43648] | 43359 | 1.1 | 376906 | 130789 | 677.1 | 677.1 | 17.725 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 390465.742, 353537.243, 377514.354, 357030.779, 384948.198, 399654.230, 362233.791, 428541.883, 382835.943, 422657.114, 414032.927, 426246.668
- **current-loop SQL, resident connection:** 53028.799, 54866.274, 54973.626, 54006.050, 53209.442, 55605.847, 54587.625, 55004.501, 53699.346, 54225.784, 54353.385, 54076.060
- **guarded prepared/hoisted SQL:** 169113.998, 164537.936, 164104.927, 161035.411, 160470.548, 162099.509, 165795.503, 166618.933, 159232.584, 164825.586, 162190.201, 164102.416
- **Nimbus-shaped SQL lower bound:** 179620.469, 180635.251, 179958.466, 176538.199, 179078.144, 185548.487, 178070.381, 179295.484, 191630.681, 187271.272, 175568.889, 183418.558
- **production storage append+apply:** 43482.196, 43338.375, 41960.890, 43965.893, 43797.072, 43378.860, 43651.810, 43496.049, 43156.663, 43329.077, 43195.426, 43242.388

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **829212 logical mutations/s** (0.926 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 39.3 |
| production-equivalent connection init on initialized DB | 447.4 |
| `SqliteTenantStore::open` + schema load | 408.9 |

