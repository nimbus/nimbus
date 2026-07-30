# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 384979 | [384550, 385408] | 385057 | 0.2 | 390994 | 384979 | 3007.6 | 3007.6 | 1.995 ms |
| current-loop SQL, resident connection | 19007 | [18944, 19071] | 19057 | 0.5 | 159607 | 57319 | 297.0 | 297.0 | 40.406 ms |
| guarded prepared/hoisted SQL | 114603 | [114468, 114738] | 114629 | 0.2 | 507504 | 345598 | 1790.7 | 1790.7 | 6.701 ms |
| Nimbus-shaped SQL lower bound | 140344 | [140170, 140518] | 140343 | 0.2 | 429804 | 423225 | 2192.9 | 2192.9 | 5.472 ms |
| production storage append+apply | 72370 | [72113, 72627] | 72313 | 0.6 | 629469 | 218429 | 1130.8 | 1130.8 | 10.612 ms |
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
| guarded minus binding cleanup (preimage kept) | 116256 | [116004, 116508] | 0.3 | 6.606 ms |
| guarded minus preimage (binding kept) | 135492 | [135329, 135656] | 0.2 | 5.668 ms |
| guarded plus preimage decode+compare | 103722 | [103570, 103873] | 0.2 | 7.404 ms |

## Raw measured-round samples

- **raw row mutation:** 385009.183, 384720.039, 385105.278, 384810.671, 383205.258, 385196.262, 385616.998, 384814.216, 384637.399, 385458.211, 385893.648, 385281.517
- **current-loop SQL, resident connection:** 18830.172, 18815.980, 18930.417, 19101.014, 19069.363, 19056.622, 18953.547, 19047.525, 19073.828, 19057.407, 19080.794, 19071.698
- **guarded prepared/hoisted SQL:** 114342.441, 114858.072, 114427.824, 114313.274, 114656.217, 114886.171, 114613.260, 114391.476, 114496.035, 114923.033, 114645.628, 114677.434
- **Nimbus-shaped SQL lower bound:** 140524.208, 140212.904, 140207.126, 140485.265, 140406.855, 139903.948, 140085.963, 140830.935, 140015.986, 140279.783, 140615.706, 140561.309
- **production storage append+apply:** 72491.631, 73041.586, 72944.186, 72434.370, 72351.441, 71999.173, 72026.197, 71752.029, 72039.961, 72847.193, 72238.123, 72274.685
- **guarded minus binding cleanup (preimage kept):** 115803.588, 116779.425, 116864.811, 116625.224, 116218.786, 116174.301, 116677.606, 116139.703, 116017.853, 115601.380, 116102.342, 116070.224
- **guarded minus preimage (binding kept):** 135276.029, 135399.165, 135350.991, 135259.967, 135509.384, 135701.513, 135435.551, 135863.237, 135765.467, 135566.754, 135784.033, 134993.801
- **guarded plus preimage decode+compare:** 103895.833, 103740.811, 103985.335, 103492.407, 104109.888, 103870.880, 103685.160, 103187.690, 103699.029, 103695.127, 103683.099, 103618.098

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **381280 logical mutations/s** (2.014 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 38.9 |
| production-equivalent connection init on initialized DB | 449.1 |
| `SqliteTenantStore::open` + schema load | 555.4 |

