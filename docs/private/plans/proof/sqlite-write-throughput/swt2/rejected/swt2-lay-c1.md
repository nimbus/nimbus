# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 319562 | [311974, 327150] | 318377 | 3.7 | 324555 | 319562 | 2496.6 | 2496.6 | 2.406 ms |
| current-loop SQL, resident connection | 47931 | [47491, 48372] | 47961 | 1.4 | 402487 | 144543 | 748.9 | 748.9 | 16.026 ms |
| guarded prepared/hoisted SQL | 133837 | [123030, 144643] | 138587 | 12.7 | 592680 | 403601 | 2091.2 | 2091.2 | 5.875 ms |
| Nimbus-shaped SQL lower bound | 153032 | [150874, 155190] | 152650 | 2.2 | 468660 | 461487 | 2391.1 | 2391.1 | 5.021 ms |
| production storage append+apply | 103332 | [102542, 104121] | 102852 | 1.2 | 898770 | 311878 | 1614.6 | 1614.6 | 7.433 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 325055.851, 316809.454, 304677.439, 340080.919, 318404.944, 312051.629, 303931.975, 330488.818, 332210.175, 318348.296, 304408.481, 328278.977
- **current-loop SQL, resident connection:** 48131.402, 49330.442, 48044.612, 47877.992, 47166.074, 46903.065, 47044.629, 47638.145, 48617.525, 48445.361, 47877.399, 48100.772
- **guarded prepared/hoisted SQL:** 79996.603, 141911.335, 138770.361, 138493.341, 139556.895, 137624.388, 136346.102, 137849.834, 138681.162, 139690.686, 138308.117, 138810.285
- **Nimbus-shaped SQL lower bound:** 153088.238, 152841.376, 149107.170, 157546.045, 149406.028, 153283.747, 148962.289, 155637.337, 152148.108, 151692.406, 152457.983, 160213.270
- **production storage append+apply:** 102146.458, 104069.644, 106627.000, 102740.987, 102067.448, 102835.662, 103058.505, 103549.796, 102795.628, 102842.356, 104384.494, 102861.400

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **762347 logical mutations/s** (1.007 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 41.2 |
| production-equivalent connection init on initialized DB | 573.5 |
| `SqliteTenantStore::open` + schema load | 621.8 |

