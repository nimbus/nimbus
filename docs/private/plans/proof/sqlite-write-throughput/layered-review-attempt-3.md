# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 309697 | [298021, 321373] | 310803 | 5.9 | 314536 | 309697 | 2411.4 | 2411.4 | 2.488 ms |
| current-loop SQL, resident connection | 47581 | [45467, 49695] | 48424 | 7.0 | 399545 | 143487 | 739.6 | 739.6 | 16.226 ms |
| guarded prepared/hoisted SQL | 129216 | [115288, 143144] | 137177 | 17.0 | 572218 | 389667 | 1959.2 | 1959.2 | 6.125 ms |
| Nimbus-shaped SQL lower bound | 142433 | [128705, 156160] | 138932 | 15.2 | 436200 | 429523 | 2169.5 | 2169.5 | 5.531 ms |
| production storage append+apply | 34387 | [33440, 35333] | 34334 | 4.3 | 299091 | 103787 | 536.4 | 536.4 | 22.372 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 708672 | 4096 | 172 | 172 | 1000 |
| current-loop SQL, resident connection | 4096 | 1400832 | 4096 | 340 | 340 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1400832 | 4096 | 340 | 340 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1400832 | 4096 | 340 | 340 | 1000 |
| production storage append+apply | 4096 | 1392592 | 4096 | 338 | 338 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 282124.453, 322647.285, 325747.802, 307013.154, 276549.146, 324158.428, 335038.433, 305829.805, 305255.690, 292717.557, 324690.913, 314592.481
- **current-loop SQL, resident connection:** 50753.163, 49222.890, 45339.892, 38094.587, 49307.856, 46901.722, 49350.589, 47728.982, 48687.360, 50001.305, 48159.988, 47425.175
- **guarded prepared/hoisted SQL:** 145497.791, 112028.303, 127443.334, 95657.145, 134456.038, 109925.620, 91130.183, 157849.437, 144033.196, 144575.271, 148098.094, 139897.259
- **Nimbus-shaped SQL lower bound:** 161768.241, 156767.002, 129274.476, 169601.941, 165302.275, 157680.330, 140872.413, 92244.718, 136678.245, 135653.944, 136990.883, 126355.593
- **production storage append+apply:** 33315.443, 34637.367, 31690.151, 34447.875, 33157.690, 34378.008, 34290.301, 33522.962, 35316.203, 36803.866, 37001.829, 34076.994

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **629099 logical mutations/s** (1.221 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 51.2 |
| production-equivalent connection init on initialized DB | 664.8 |
| `SqliteTenantStore::open` + schema load | 689.4 |

