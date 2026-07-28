# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 328753 | [318204, 339302] | 327952 | 5.1 | 333890 | 328753 | 2568.4 | 2568.4 | 2.342 ms |
| current-loop SQL, resident connection | 46522 | [45608, 47436] | 46898 | 3.1 | 390650 | 140292 | 726.9 | 726.9 | 16.523 ms |
| guarded prepared/hoisted SQL | 132667 | [129599, 135736] | 134751 | 3.6 | 587503 | 400075 | 2072.9 | 2072.9 | 5.796 ms |
| Nimbus-shaped SQL lower bound | 149187 | [145292, 153082] | 149992 | 4.1 | 456886 | 449893 | 2331.1 | 2331.1 | 5.156 ms |
| production storage append+apply | 75656 | [74891, 76422] | 75633 | 1.6 | 658053 | 228348 | 1182.1 | 1182.1 | 10.154 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 294895.882, 337385.002, 328519.159, 343624.902, 327384.533, 340751.952, 324903.353, 316195.043, 309154.636, 338513.028, 356839.089, 326873.143
- **current-loop SQL, resident connection:** 43670.455, 44825.462, 46428.490, 47608.572, 44565.838, 46617.584, 47337.766, 47513.729, 47860.281, 47177.647, 46441.298, 48214.948
- **guarded prepared/hoisted SQL:** 135621.987, 135202.772, 136164.647, 133347.544, 136990.849, 135466.725, 134299.372, 125987.754, 136152.358, 132975.597, 128217.007, 121583.354
- **Nimbus-shaped SQL lower bound:** 147099.412, 139271.618, 146842.023, 138065.898, 148685.891, 152408.630, 150845.538, 149659.546, 160710.012, 152134.164, 150323.571, 154202.287
- **production storage append+apply:** 75504.928, 77611.204, 76013.587, 75241.769, 73755.757, 74284.141, 77286.184, 75760.954, 74263.080, 76377.748, 75151.959, 76625.862

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **754012 logical mutations/s** (1.019 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 40.6 |
| production-equivalent connection init on initialized DB | 419.9 |
| `SqliteTenantStore::open` + schema load | 452.7 |

