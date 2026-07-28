# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 310675 | [303266, 318083] | 310573 | 3.8 | 315529 | 310675 | 2424.0 | 2424.0 | 2.475 ms |
| current-loop SQL, resident connection | 45416 | [42502, 48329] | 45043 | 10.1 | 381361 | 136956 | 702.5 | 702.5 | 17.083 ms |
| guarded prepared/hoisted SQL | 144358 | [137073, 151643] | 144549 | 7.9 | 639274 | 435330 | 2241.4 | 2241.4 | 5.354 ms |
| Nimbus-shaped SQL lower bound | 170396 | [166063, 174730] | 171579 | 4.0 | 521838 | 513851 | 2658.3 | 2658.3 | 4.514 ms |
| production storage append+apply | 38318 | [37158, 39479] | 38855 | 4.8 | 333290 | 115654 | 597.4 | 597.4 | 20.088 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |
| production storage append+apply | 4096 | 1388472 | 4096 | 337 | 337 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 305240.695, 330402.718, 309560.652, 316643.720, 301069.890, 324964.732, 293371.360, 311585.480, 304043.690, 315751.435, 294386.650, 321077.518
- **current-loop SQL, resident connection:** 45132.976, 44946.094, 36660.036, 44267.201, 44953.813, 38145.853, 42560.059, 49155.950, 49480.956, 50599.575, 49930.420, 49154.034
- **guarded prepared/hoisted SQL:** 136718.139, 153924.206, 117324.577, 156290.918, 140660.646, 152881.669, 148437.067, 138969.170, 137378.345, 155894.663, 153846.156, 139972.351
- **Nimbus-shaped SQL lower bound:** 171304.324, 166466.796, 174407.436, 169763.615, 180665.469, 175050.770, 171854.155, 172589.953, 169390.100, 153554.683, 164391.259, 175315.978
- **production storage append+apply:** 39306.279, 39995.880, 39678.232, 39613.479, 38129.425, 39003.640, 36466.825, 38865.704, 38844.093, 33379.955, 37776.520, 38760.701

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **646096 logical mutations/s** (1.189 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 43.6 |
| production-equivalent connection init on initialized DB | 510.9 |
| `SqliteTenantStore::open` + schema load | 504.0 |

