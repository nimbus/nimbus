# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 406675 | [398438, 414912] | 411221 | 3.2 | 413029 | 406675 | 3177.1 | 3177.1 | 1.890 ms |
| current-loop SQL, resident connection | 53318 | [52890, 53746] | 53341 | 1.3 | 447718 | 160787 | 833.1 | 833.1 | 14.406 ms |
| guarded prepared/hoisted SQL | 165425 | [161915, 168934] | 166881 | 3.3 | 732564 | 498859 | 2584.8 | 2584.8 | 4.648 ms |
| Nimbus-shaped SQL lower bound | 179785 | [171891, 187679] | 185327 | 6.9 | 550591 | 542163 | 2809.1 | 2809.1 | 4.294 ms |
| production storage append+apply | 121939 | [119756, 124123] | 122742 | 2.8 | 1060618 | 368041 | 1905.3 | 1905.3 | 6.303 ms |
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
| guarded minus binding cleanup (preimage kept) | 169197 | [168001, 170392] | 1.1 | 4.540 ms |
| guarded minus preimage (binding kept) | 183684 | [182453, 184915] | 1.1 | 4.182 ms |
| guarded plus preimage decode+compare | 158500 | [155806, 161193] | 2.7 | 4.849 ms |

## Raw measured-round samples

- **raw row mutation:** 416776.103, 421950.329, 396216.824, 414363.173, 412878.557, 413952.956, 383022.772, 417130.130, 407948.711, 403365.606, 409564.206, 382932.575
- **current-loop SQL, resident connection:** 52626.164, 53912.421, 54017.650, 53263.641, 53250.786, 52653.500, 53343.647, 53598.854, 54032.374, 51824.624, 53954.107, 53337.943
- **guarded prepared/hoisted SQL:** 149527.274, 169135.027, 162984.273, 163355.791, 166648.960, 169630.738, 167113.219, 168832.545, 169445.509, 166560.766, 164024.775, 167836.754
- **Nimbus-shaped SQL lower bound:** 186406.888, 177819.799, 188179.305, 189731.915, 185289.590, 185602.516, 187670.516, 173810.903, 171912.099, 185365.276, 180835.776, 144791.906
- **production storage append+apply:** 124427.896, 126244.326, 123451.113, 123316.886, 122714.226, 119561.091, 112622.092, 120180.793, 122579.542, 122769.139, 121563.588, 123840.907
- **guarded minus binding cleanup (preimage kept):** 169889.945, 171575.459, 171939.231, 166615.647, 170243.213, 171081.092, 168345.011, 168749.564, 167791.630, 165872.044, 169244.541, 169012.354
- **guarded minus preimage (binding kept):** 185052.499, 182722.911, 179497.092, 183999.535, 184284.891, 184934.106, 184772.665, 186931.210, 183440.734, 184292.108, 183146.521, 181134.150
- **guarded plus preimage decode+compare:** 156839.295, 160291.013, 160630.181, 159297.865, 159901.451, 159920.479, 157918.093, 159863.563, 162117.474, 145688.583, 159790.137, 159736.219

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **829469 logical mutations/s** (0.926 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 40.0 |
| production-equivalent connection init on initialized DB | 449.3 |
| `SqliteTenantStore::open` + schema load | 419.5 |

