# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 408490 | [396599, 420381] | 410448 | 4.6 | 414873 | 408490 | 3191.3 | 3191.3 | 1.884 ms |
| current-loop SQL, resident connection | 54520 | [54195, 54845] | 54560 | 0.9 | 457812 | 164412 | 851.9 | 851.9 | 14.088 ms |
| guarded prepared/hoisted SQL | 165545 | [162884, 168205] | 165657 | 2.5 | 733096 | 499221 | 2586.6 | 2586.6 | 4.642 ms |
| Nimbus-shaped SQL lower bound | 182042 | [178613, 185471] | 181989 | 3.0 | 557503 | 548970 | 2844.4 | 2844.4 | 4.222 ms |
| production storage append+apply | 91324 | [90422, 92226] | 90945 | 1.6 | 794329 | 275637 | 1426.9 | 1426.9 | 8.411 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 412269.517, 406180.541, 424879.278, 407277.919, 425151.243, 356891.025, 418158.007, 408625.655, 401258.111, 423691.345, 397659.302, 419841.475
- **current-loop SQL, resident connection:** 54949.868, 54906.033, 53809.326, 54074.947, 54827.928, 54006.487, 53962.382, 54292.189, 54178.158, 55021.410, 54934.635, 55276.831
- **guarded prepared/hoisted SQL:** 169352.121, 169114.517, 159199.029, 171784.537, 169100.685, 167456.769, 163784.000, 163401.987, 166812.628, 164074.130, 157955.916, 164501.541
- **Nimbus-shaped SQL lower bound:** 184586.388, 175094.451, 190003.412, 190518.093, 180164.241, 182679.265, 181298.094, 174853.715, 185628.091, 185326.417, 175905.052, 178444.478
- **production storage append+apply:** 93388.589, 93524.704, 93264.508, 91426.841, 89821.585, 89337.212, 90964.582, 90924.913, 90700.791, 90681.989, 90005.582, 91847.749

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **825317 logical mutations/s** (0.931 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 39.4 |
| production-equivalent connection init on initialized DB | 441.8 |
| `SqliteTenantStore::open` + schema load | 412.3 |

