# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 228949 | [194899, 262999] | 252173 | 23.4 | 232526 | 228949 | 1788.7 | 1788.7 | 3.581 ms |
| current-loop SQL, resident connection | 41957 | [34916, 48999] | 47932 | 26.4 | 352323 | 126528 | 655.6 | 655.6 | 20.306 ms |
| guarded prepared/hoisted SQL | 131474 | [107780, 155169] | 144473 | 28.4 | 582219 | 396478 | 2054.3 | 2054.3 | 7.371 ms |
| Nimbus-shaped SQL lower bound | 163233 | [146832, 179635] | 175047 | 15.8 | 499902 | 492251 | 2550.5 | 2550.5 | 4.834 ms |
| production storage append+apply | 40367 | [39137, 41598] | 41025 | 4.8 | 351113 | 121838 | 630.7 | 630.7 | 19.067 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 201203.963, 167740.677, 166108.600, 224981.872, 300770.364, 262085.847, 249428.486, 271253.754, 255685.249, 254917.665, 271318.034, 121893.850
- **current-loop SQL, resident connection:** 25464.409, 32485.580, 17623.057, 41873.831, 40322.653, 49459.889, 49900.167, 50983.488, 48041.718, 47822.825, 49214.251, 50297.850
- **guarded prepared/hoisted SQL:** 138475.044, 28123.907, 110779.849, 153208.806, 159855.500, 151110.020, 144586.102, 145216.169, 102778.921, 144359.283, 164902.090, 134297.478
- **Nimbus-shaped SQL lower bound:** 147865.035, 151703.933, 108404.951, 190794.548, 183491.748, 185219.919, 171841.258, 139884.235, 185833.145, 178796.817, 136712.934, 178252.780
- **production storage append+apply:** 42651.415, 41764.250, 42275.464, 41488.748, 40261.138, 36646.792, 38017.181, 41154.997, 41186.900, 40570.243, 40894.904, 37497.931

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **837201 logical mutations/s** (0.917 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 47.9 |
| production-equivalent connection init on initialized DB | 460.1 |
| `SqliteTenantStore::open` + schema load | 468.2 |

