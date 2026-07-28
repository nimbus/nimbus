# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 403399 | [394012, 412787] | 404752 | 3.7 | 409702 | 403399 | 3151.6 | 3151.6 | 1.906 ms |
| current-loop SQL, resident connection | 53599 | [53323, 53876] | 53549 | 0.8 | 450080 | 161635 | 837.5 | 837.5 | 14.329 ms |
| guarded prepared/hoisted SQL | 166797 | [163984, 169610] | 168009 | 2.7 | 738642 | 502998 | 2606.2 | 2606.2 | 4.607 ms |
| Nimbus-shaped SQL lower bound | 185919 | [181801, 190037] | 187460 | 3.5 | 569377 | 560663 | 2905.0 | 2905.0 | 4.136 ms |
| production storage append+apply | 122911 | [119836, 125985] | 124811 | 3.9 | 1069067 | 370973 | 1920.5 | 1920.5 | 6.258 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## SWT4.1 forward-apply attribution (diagnostic)

Each lane replays the guarded fixture with one component toggled. `decode+compare` additionally deserializes both preimage JSON columns and compares fields against the record's previous document, pricing production's Rust-side work that the guarded lane black-boxes. Deltas attribute the combined guarded-to-lower-bound gap.

| lane | logical mut/s | 95% CI | CV% | mean elapsed |
|---|---:|---:|---:|---:|
| guarded minus binding cleanup (preimage kept) | 171135 | [169444, 172827] | 1.6 | 4.489 ms |
| guarded minus preimage (binding kept) | 173135 | [145132, 201139] | 25.5 | 5.696 ms |
| guarded plus preimage decode+compare | 161019 | [159376, 162663] | 1.6 | 4.771 ms |

## Raw measured-round samples

- **raw row mutation:** 403551.074, 407823.250, 383184.398, 394894.575, 381973.690, 405953.033, 385128.098, 426295.639, 417328.773, 412159.820, 420814.555, 401684.144
- **current-loop SQL, resident connection:** 53853.213, 53253.469, 53285.952, 52831.960, 53998.800, 53647.982, 54073.305, 54205.401, 53410.866, 53141.337, 53450.777, 54038.358
- **guarded prepared/hoisted SQL:** 157666.393, 164364.066, 170678.862, 168037.478, 171376.133, 167979.945, 169204.303, 165655.907, 162905.022, 168069.019, 173357.396, 162270.926
- **Nimbus-shaped SQL lower bound:** 193181.561, 185868.940, 187814.159, 185823.810, 166626.716, 187388.486, 189656.429, 187651.756, 189171.787, 187532.303, 185403.248, 184910.916
- **production storage append+apply:** 126718.731, 126030.568, 125788.725, 110971.769, 122174.294, 125180.850, 122563.352, 127437.857, 125683.811, 121711.609, 116224.910, 124441.758
- **guarded minus binding cleanup (preimage kept):** 176793.486, 168741.708, 172098.354, 170080.829, 168969.154, 170598.161, 171352.048, 167538.454, 170525.246, 169479.108, 175086.884, 172361.692
- **guarded minus preimage (binding kept):** 179659.451, 187810.424, 186085.116, 180272.495, 191795.255, 185993.133, 187546.291, 33575.808, 184343.047, 186917.311, 185475.076, 188148.091
- **guarded plus preimage decode+compare:** 160273.406, 157541.717, 159360.160, 163079.351, 165112.763, 160721.317, 159712.717, 157771.680, 159084.480, 162238.861, 161996.905, 165338.655

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **819679 logical mutations/s** (0.937 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 40.4 |
| production-equivalent connection init on initialized DB | 485.1 |
| `SqliteTenantStore::open` + schema load | 434.3 |

