# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 394941 | [360052, 429830] | 415423 | 13.9 | 401112 | 394941 | 3085.5 | 3085.5 | 2.002 ms |
| current-loop SQL, resident connection | 53847 | [53154, 54540] | 54218 | 2.0 | 452164 | 162383 | 841.4 | 841.4 | 14.268 ms |
| guarded prepared/hoisted SQL | 167874 | [165657, 170092] | 168039 | 2.1 | 743413 | 506246 | 2623.0 | 2623.0 | 4.577 ms |
| Nimbus-shaped SQL lower bound | 178545 | [162115, 194975] | 186154 | 14.5 | 546794 | 538425 | 2789.8 | 2789.8 | 4.447 ms |
| production storage append+apply | 42996 | [42841, 43150] | 43040 | 0.6 | 373973 | 129771 | 671.8 | 671.8 | 17.863 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 223276.864, 415974.268, 415091.184, 401929.274, 417720.331, 393849.934, 420972.161, 406104.956, 417399.025, 393064.094, 418155.150, 415754.396
- **current-loop SQL, resident connection:** 54602.550, 54483.864, 53938.240, 51015.503, 53953.534, 54266.385, 54528.240, 54453.460, 53772.944, 54168.682, 52330.425, 54654.474
- **guarded prepared/hoisted SQL:** 169282.885, 171561.938, 173420.353, 172184.093, 168873.637, 167569.274, 167057.326, 168508.125, 162144.594, 163563.012, 165794.159, 164533.796
- **Nimbus-shaped SQL lower bound:** 187620.845, 185011.856, 185661.033, 186957.630, 184051.594, 182268.447, 186823.970, 182593.793, 96735.534, 188845.493, 189320.906, 186647.872
- **production storage append+apply:** 43072.161, 43030.718, 43332.049, 43262.506, 42985.120, 43019.101, 43116.571, 42412.955, 43049.302, 43099.092, 42772.811, 42796.011

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **817848 logical mutations/s** (0.939 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 39.0 |
| production-equivalent connection init on initialized DB | 411.1 |
| `SqliteTenantStore::open` + schema load | 419.5 |

