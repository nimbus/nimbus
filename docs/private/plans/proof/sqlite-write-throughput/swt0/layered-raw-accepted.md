# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 404859 | [390916, 418802] | 412412 | 5.4 | 411185 | 404859 | 3163.0 | 3163.0 | 1.903 ms |
| current-loop SQL, resident connection | 54169 | [53785, 54552] | 54293 | 1.1 | 454861 | 163352 | 846.4 | 846.4 | 14.180 ms |
| guarded prepared/hoisted SQL | 169972 | [168498, 171447] | 169320 | 1.4 | 752703 | 512573 | 2655.8 | 2655.8 | 4.519 ms |
| Nimbus-shaped SQL lower bound | 185940 | [182725, 189156] | 187111 | 2.7 | 569442 | 560727 | 2905.3 | 2905.3 | 4.133 ms |
| production storage append+apply | 42806 | [42482, 43129] | 42971 | 1.2 | 372319 | 129197 | 668.8 | 668.8 | 17.944 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 421230.962, 425005.517, 405108.538, 420290.157, 416445.434, 404236.702, 374096.184, 403015.053, 411299.868, 349307.130, 413524.955, 414748.410
- **current-loop SQL, resident connection:** 54891.082, 54631.457, 54226.119, 54262.149, 54333.883, 54354.157, 54240.763, 54323.338, 54047.577, 53771.560, 52466.488, 54474.913
- **guarded prepared/hoisted SQL:** 171314.940, 172293.617, 173762.369, 173691.365, 168819.062, 170210.383, 169099.885, 169540.686, 166684.323, 168034.517, 168096.835, 168119.298
- **Nimbus-shaped SQL lower bound:** 189059.957, 184843.542, 186839.624, 193412.413, 175373.078, 182414.469, 178525.853, 190819.472, 187936.876, 186516.983, 188159.426, 187383.058
- **production storage append+apply:** 41441.555, 43230.562, 42274.908, 43005.727, 42937.206, 42518.868, 43053.607, 42936.364, 43184.821, 42900.566, 43071.093, 43111.055

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **834668 logical mutations/s** (0.920 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 38.9 |
| production-equivalent connection init on initialized DB | 459.2 |
| `SqliteTenantStore::open` + schema load | 424.5 |

