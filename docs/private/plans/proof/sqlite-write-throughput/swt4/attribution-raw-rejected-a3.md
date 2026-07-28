# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 357267 | [339890, 374643] | 360181 | 7.7 | 362849 | 357267 | 2791.1 | 2791.1 | 2.162 ms |
| current-loop SQL, resident connection | 45695 | [43832, 47557] | 45796 | 6.4 | 383703 | 137798 | 714.0 | 714.0 | 16.875 ms |
| guarded prepared/hoisted SQL | 141310 | [136319, 146300] | 142394 | 5.6 | 625773 | 426137 | 2208.0 | 2208.0 | 5.451 ms |
| Nimbus-shaped SQL lower bound | 158068 | [148676, 167460] | 161650 | 9.4 | 484083 | 476674 | 2469.8 | 2469.8 | 4.900 ms |
| production storage append+apply | 96855 | [83142, 110568] | 105143 | 22.3 | 842434 | 292330 | 1513.4 | 1513.4 | 8.507 ms |
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
| guarded minus binding cleanup (preimage kept) | 145056 | [134072, 156039] | 11.9 | 5.387 ms |
| guarded minus preimage (binding kept) | 160353 | [154079, 166627] | 6.2 | 4.807 ms |
| guarded plus preimage decode+compare | 133850 | [127709, 139991] | 7.2 | 5.766 ms |

## Raw measured-round samples

- **raw row mutation:** 330149.816, 385762.436, 361279.522, 332801.491, 297837.398, 378120.745, 354344.218, 377942.154, 359082.877, 342292.383, 380065.919, 387519.836
- **current-loop SQL, resident connection:** 48267.065, 49296.520, 47494.425, 48960.380, 45399.408, 43082.815, 46137.349, 39008.794, 45454.060, 43343.352, 47277.942, 44612.309
- **guarded prepared/hoisted SQL:** 150150.769, 138831.145, 143129.687, 140419.700, 146689.207, 150979.416, 141872.795, 126721.696, 128729.152, 142914.890, 149351.990, 135924.545
- **Nimbus-shaped SQL lower bound:** 158978.805, 164322.062, 170267.276, 144368.497, 170556.357, 144046.551, 143529.660, 155450.379, 174437.447, 170247.535, 171746.764, 128864.917
- **production storage append+apply:** 113150.560, 103031.671, 109366.234, 102693.686, 110074.561, 112646.090, 107254.434, 87418.985, 55504.110, 109277.933, 101092.675, 50745.159
- **guarded minus binding cleanup (preimage kept):** 97249.999, 153619.286, 155731.138, 149336.750, 156204.867, 157373.089, 152431.194, 136030.424, 148379.812, 152334.816, 129011.110, 152963.713
- **guarded minus preimage (binding kept):** 165994.111, 165151.253, 168609.114, 146572.849, 169716.177, 142645.664, 169025.634, 148637.947, 169667.152, 165430.932, 157422.545, 155366.604
- **guarded plus preimage decode+compare:** 140865.881, 136248.474, 132355.745, 122401.519, 140036.937, 120941.403, 122568.513, 133191.367, 143218.768, 122710.686, 147811.933, 143846.937

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **475794 logical mutations/s** (1.614 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 43.1 |
| production-equivalent connection init on initialized DB | 564.7 |
| `SqliteTenantStore::open` + schema load | 761.1 |

