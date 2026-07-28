# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 403010 | [389902, 416118] | 408945 | 5.1 | 409307 | 403010 | 3148.5 | 3148.5 | 1.910 ms |
| current-loop SQL, resident connection | 53827 | [53386, 54267] | 53833 | 1.3 | 451990 | 162321 | 841.0 | 841.0 | 14.270 ms |
| guarded prepared/hoisted SQL | 161106 | [155462, 166750] | 165512 | 5.5 | 713439 | 485835 | 2517.3 | 2517.3 | 4.781 ms |
| Nimbus-shaped SQL lower bound | 184218 | [181879, 186556] | 185507 | 2.0 | 564167 | 555532 | 2878.4 | 2878.4 | 4.171 ms |
| production storage append+apply | 43119 | [42931, 43307] | 43049 | 0.7 | 375043 | 130142 | 673.7 | 673.7 | 17.812 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 426861.485, 427519.219, 395658.744, 420396.875, 372566.975, 422234.351, 374457.832, 407694.108, 392494.523, 411190.415, 410195.213, 374848.996
- **current-loop SQL, resident connection:** 54179.175, 53621.204, 52915.648, 53522.109, 52419.414, 53598.111, 53916.230, 54173.320, 54733.219, 54252.566, 53749.352, 54839.820
- **guarded prepared/hoisted SQL:** 164529.831, 166849.598, 149170.727, 168221.713, 149516.054, 149082.754, 155851.952, 166495.116, 168936.813, 168511.718, 153811.446, 172294.286
- **Nimbus-shaped SQL lower bound:** 187405.222, 187127.952, 186484.310, 184103.460, 177301.406, 184885.655, 187723.521, 177848.565, 181383.668, 186129.083, 187435.494, 182785.183
- **production storage append+apply:** 42646.612, 43057.126, 43023.464, 42971.110, 42844.333, 43129.303, 43280.795, 43040.312, 43649.965, 43673.706, 43077.037, 43031.488

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **833884 logical mutations/s** (0.921 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 39.3 |
| production-equivalent connection init on initialized DB | 447.5 |
| `SqliteTenantStore::open` + schema load | 417.1 |

