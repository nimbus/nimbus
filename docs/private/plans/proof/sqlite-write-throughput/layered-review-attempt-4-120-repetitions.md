# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 120; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 267326 | [242225, 292428] | 277236 | 14.8 | 271503 | 267326 | 2040.7 | 2040.7 | 2.940 ms |
| current-loop SQL, resident connection | 46120 | [44980, 47260] | 46725 | 3.9 | 387274 | 139080 | 719.6 | 719.6 | 16.677 ms |
| guarded prepared/hoisted SQL | 133993 | [131594, 136391] | 134410 | 2.8 | 593371 | 404072 | 2092.1 | 2092.1 | 5.736 ms |
| Nimbus-shaped SQL lower bound | 140666 | [121121, 160211] | 151739 | 21.9 | 430791 | 424197 | 1992.8 | 1992.8 | 6.022 ms |
| production storage append+apply | 35733 | [34923, 36542] | 36121 | 3.6 | 310800 | 107850 | 557.7 | 557.7 | 21.519 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |
| production storage append+apply | 4096 | 1388472 | 4096 | 337 | 337 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 276962.820, 190133.440, 266723.068, 263089.623, 277508.725, 297599.767, 319947.378, 304548.107, 206882.254, 229628.899, 291906.758, 282985.920
- **current-loop SQL, resident connection:** 45021.303, 46353.443, 41391.200, 47226.861, 44244.629, 47663.745, 46547.004, 46902.131, 47406.622, 46388.567, 47018.969, 47272.287
- **guarded prepared/hoisted SQL:** 134168.685, 133867.113, 136229.524, 125235.938, 136297.621, 135891.530, 134356.379, 128573.207, 138701.687, 137502.242, 132624.440, 134464.293
- **Nimbus-shaped SQL lower bound:** 111050.912, 51263.856, 151097.140, 159078.362, 155711.829, 145725.703, 154979.360, 152696.934, 153902.547, 152380.427, 149406.179, 150702.750
- **production storage append+apply:** 37006.987, 36008.178, 36379.461, 35367.516, 36130.230, 36801.804, 36112.347, 37031.446, 36485.673, 34235.305, 33022.913, 34211.109

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **636577 logical mutations/s** (1.206 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 51.5 |
| production-equivalent connection init on initialized DB | 666.5 |
| `SqliteTenantStore::open` + schema load | 683.6 |

