# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 400496 | [393139, 407853] | 401957 | 2.9 | 406754 | 400496 | 3128.9 | 3128.9 | 1.919 ms |
| current-loop SQL, resident connection | 52753 | [52557, 52949] | 52784 | 0.6 | 442976 | 159084 | 824.3 | 824.3 | 14.559 ms |
| guarded prepared/hoisted SQL | 164478 | [163396, 165560] | 164809 | 1.0 | 728372 | 496004 | 2570.0 | 2570.0 | 4.670 ms |
| Nimbus-shaped SQL lower bound | 181046 | [175695, 186398] | 183908 | 4.7 | 554455 | 545968 | 2828.9 | 2828.9 | 4.251 ms |
| production storage append+apply | 115214 | [106382, 124047] | 118687 | 12.1 | 1002125 | 347743 | 1800.2 | 1800.2 | 6.801 ms |
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
| guarded minus binding cleanup (preimage kept) | 134297 | [115525, 153068] | 22.0 | 6.066 ms |
| guarded minus preimage (binding kept) | 170570 | [162709, 178430] | 7.3 | 4.526 ms |
| guarded plus preimage decode+compare | 156853 | [154250, 159456] | 2.6 | 4.899 ms |

## Raw measured-round samples

- **raw row mutation:** 411410.510, 414768.136, 391297.014, 400743.118, 408919.064, 411065.516, 375490.221, 399651.235, 403170.051, 395645.037, 406950.758, 386842.766
- **current-loop SQL, resident connection:** 52086.169, 52725.719, 52287.174, 52874.402, 52726.822, 53053.694, 52790.850, 52795.827, 53148.248, 52685.053, 52776.757, 53088.335
- **guarded prepared/hoisted SQL:** 166105.207, 162973.418, 166726.895, 164087.788, 160317.920, 164781.770, 164311.001, 164953.643, 165734.479, 165557.997, 163348.622, 164836.270
- **Nimbus-shaped SQL lower bound:** 185707.608, 174981.475, 157162.528, 182443.717, 185398.247, 181469.745, 183584.282, 177411.591, 185348.065, 184232.214, 186714.017, 188103.834
- **production storage append+apply:** 116930.759, 121649.384, 122237.965, 121941.149, 121495.771, 122024.594, 115142.620, 114778.358, 118454.107, 118918.994, 117063.443, 71935.385
- **guarded minus binding cleanup (preimage kept):** 70884.601, 121636.162, 163941.011, 147504.578, 136762.096, 159272.511, 164647.849, 164062.155, 146384.526, 119750.221, 97724.050, 118992.117
- **guarded minus preimage (binding kept):** 148751.007, 182199.864, 167868.952, 164803.893, 166093.506, 165732.663, 149005.178, 177701.993, 184129.948, 177457.571, 181295.629, 181795.498
- **guarded plus preimage decode+compare:** 160927.990, 158282.183, 146338.579, 160029.567, 159419.796, 153003.602, 160089.100, 157520.380, 154066.266, 157942.904, 155957.073, 158659.862

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **814396 logical mutations/s** (0.943 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 39.0 |
| production-equivalent connection init on initialized DB | 431.7 |
| `SqliteTenantStore::open` + schema load | 437.4 |

