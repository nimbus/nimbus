# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 406441 | [391969, 420913] | 417235 | 5.6 | 412792 | 406441 | 3175.3 | 3175.3 | 1.896 ms |
| current-loop SQL, resident connection | 53928 | [53438, 54419] | 53940 | 1.4 | 452842 | 162627 | 842.6 | 842.6 | 14.244 ms |
| guarded prepared/hoisted SQL | 161209 | [159007, 163411] | 162347 | 2.1 | 713895 | 486145 | 2518.9 | 2518.9 | 4.766 ms |
| Nimbus-shaped SQL lower bound | 179315 | [176830, 181801] | 180657 | 2.2 | 549153 | 540748 | 2801.8 | 2801.8 | 4.285 ms |
| production storage append+apply | 84359 | [69979, 98739] | 91469 | 26.8 | 733749 | 254615 | 1318.1 | 1318.1 | 12.712 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 393667.267, 420787.809, 419931.234, 387515.763, 347319.770, 423394.326, 408614.310, 422261.908, 425575.625, 417250.214, 417220.765, 393753.188
- **current-loop SQL, resident connection:** 54500.384, 54695.639, 54009.217, 53750.538, 53870.923, 53431.838, 53761.363, 51877.082, 54408.978, 54379.381, 54728.726, 53723.069
- **guarded prepared/hoisted SQL:** 158185.692, 162388.556, 164112.304, 160547.257, 166422.382, 163708.066, 155529.779, 158082.878, 156210.362, 163004.404, 164008.917, 162304.743
- **Nimbus-shaped SQL lower bound:** 178950.238, 180915.473, 178714.179, 175080.759, 183152.767, 181390.332, 180633.098, 184399.322, 180681.699, 171258.564, 174307.899, 182300.022
- **production storage append+apply:** 87345.191, 86294.687, 93129.532, 92677.231, 93867.353, 91236.226, 89413.486, 12908.476, 88523.973, 91702.429, 92418.787, 92792.956

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **841539 logical mutations/s** (0.913 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 46.3 |
| production-equivalent connection init on initialized DB | 457.5 |
| `SqliteTenantStore::open` + schema load | 415.6 |

