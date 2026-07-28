# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 396952 | [376070, 417834] | 410329 | 8.3 | 403154 | 396952 | 3101.2 | 3101.2 | 1.950 ms |
| current-loop SQL, resident connection | 53492 | [52940, 54043] | 53821 | 1.6 | 449177 | 161311 | 835.8 | 835.8 | 14.361 ms |
| guarded prepared/hoisted SQL | 165151 | [160353, 169949] | 166738 | 4.6 | 731353 | 498034 | 2580.5 | 2580.5 | 4.660 ms |
| Nimbus-shaped SQL lower bound | 187280 | [185900, 188660] | 187380 | 1.2 | 573545 | 564767 | 2926.3 | 2926.3 | 4.101 ms |
| production storage append+apply | 122353 | [119625, 125081] | 123811 | 3.5 | 1064216 | 369289 | 1911.8 | 1911.8 | 6.285 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 301311.787, 412763.770, 382712.847, 412860.220, 420670.185, 398334.374, 416403.579, 391341.053, 407894.544, 385292.998, 417822.608, 416016.513
- **current-loop SQL, resident connection:** 54663.462, 54039.250, 52706.601, 54263.153, 53830.531, 52496.625, 53811.596, 53510.403, 51801.929, 53964.933, 54149.215, 52662.283
- **guarded prepared/hoisted SQL:** 172047.294, 168180.296, 165484.423, 164259.117, 166646.647, 159596.395, 171667.321, 166586.233, 166829.941, 166848.140, 143644.125, 170022.839
- **Nimbus-shaped SQL lower bound:** 192114.102, 186048.359, 187416.844, 187343.950, 188276.698, 184534.707, 187228.915, 183817.013, 189131.086, 187866.705, 187847.973, 185735.237
- **production storage append+apply:** 121917.770, 122757.051, 123798.655, 109544.438, 123856.831, 124195.966, 125262.195, 123822.427, 123861.658, 126057.233, 122777.943, 120383.763

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **818782 logical mutations/s** (0.938 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 39.9 |
| production-equivalent connection init on initialized DB | 448.9 |
| `SqliteTenantStore::open` + schema load | 412.0 |

