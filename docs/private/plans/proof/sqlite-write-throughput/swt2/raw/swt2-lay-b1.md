# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 297096 | [279476, 314715] | 307228 | 9.3 | 301738 | 297096 | 2321.1 | 2321.1 | 2.608 ms |
| current-loop SQL, resident connection | 46394 | [45442, 47345] | 46888 | 3.2 | 389575 | 139906 | 724.9 | 724.9 | 16.570 ms |
| guarded prepared/hoisted SQL | 135999 | [133919, 138078] | 136231 | 2.4 | 602254 | 410121 | 2125.0 | 2125.0 | 5.650 ms |
| Nimbus-shaped SQL lower bound | 149454 | [146651, 152257] | 147595 | 3.0 | 457703 | 450697 | 2335.2 | 2335.2 | 5.143 ms |
| production storage append+apply | 73937 | [73232, 74642] | 73889 | 1.5 | 643096 | 223158 | 1155.3 | 1155.3 | 10.389 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 295580.652, 234400.832, 309308.197, 338720.070, 310702.558, 266458.172, 272716.989, 314201.006, 291122.671, 305146.955, 314471.559, 312316.439
- **current-loop SQL, resident connection:** 46072.101, 47224.147, 45514.778, 47201.957, 47008.456, 48496.946, 48230.506, 47037.539, 46767.381, 43330.961, 44906.935, 44934.489
- **guarded prepared/hoisted SQL:** 131936.423, 132055.984, 137283.427, 135987.486, 134945.787, 142803.445, 139642.924, 137228.251, 133351.452, 137702.803, 136474.801, 132570.522
- **Nimbus-shaped SQL lower bound:** 152518.875, 150164.323, 149780.492, 148823.970, 146795.109, 147475.605, 162264.423, 146124.088, 147713.929, 147249.518, 147266.084, 147271.911
- **production storage append+apply:** 73762.186, 75253.243, 75018.346, 73561.164, 72857.724, 74162.006, 74016.065, 71963.568, 76043.117, 73131.738, 74049.370, 73423.137

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **772594 logical mutations/s** (0.994 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 40.8 |
| production-equivalent connection init on initialized DB | 548.4 |
| `SqliteTenantStore::open` + schema load | 562.0 |

