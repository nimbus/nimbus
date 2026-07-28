# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 297568 | [288737, 306399] | 297465 | 4.7 | 302217 | 297568 | 2319.7 | 2319.7 | 2.587 ms |
| current-loop SQL, resident connection | 47149 | [44065, 50232] | 48040 | 10.3 | 395914 | 142183 | 727.3 | 727.3 | 16.499 ms |
| guarded prepared/hoisted SQL | 152785 | [150040, 155530] | 154189 | 2.8 | 676590 | 460741 | 2385.4 | 2385.4 | 5.031 ms |
| Nimbus-shaped SQL lower bound | 172792 | [170747, 174838] | 173931 | 1.9 | 529176 | 521076 | 2699.0 | 2699.0 | 4.446 ms |
| production storage append+apply | 38821 | [38241, 39402] | 39300 | 2.4 | 337663 | 117171 | 606.3 | 606.3 | 19.793 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 708672 | 4096 | 172 | 172 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 297400.802, 312279.141, 259580.528, 297159.245, 305575.943, 296874.065, 309418.103, 294683.876, 287657.957, 297528.343, 305936.728, 306718.709
- **current-loop SQL, resident connection:** 50877.130, 48243.519, 45097.191, 32896.439, 46367.282, 48147.432, 47932.538, 47551.669, 48830.136, 47869.512, 50132.684, 51838.825
- **guarded prepared/hoisted SQL:** 154678.517, 155653.068, 153360.609, 155575.857, 144753.833, 155346.876, 157463.541, 143452.377, 153077.262, 151677.777, 153958.744, 154418.373
- **Nimbus-shaped SQL lower bound:** 170676.387, 176617.082, 174800.868, 176447.471, 174268.484, 174243.035, 174979.197, 173618.394, 167433.088, 166688.572, 171660.733, 172072.671
- **production storage append+apply:** 39343.173, 39337.830, 39319.183, 36633.367, 39821.777, 38455.277, 38775.556, 39280.956, 37648.576, 38403.063, 39414.439, 39420.797

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **652129 logical mutations/s** (1.178 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 43.9 |
| production-equivalent connection init on initialized DB | 595.7 |
| `SqliteTenantStore::open` + schema load | 608.8 |

