# IMV0 fail-before evidence

Date: 2026-08-20  
Baseline: `137cc632a1c8585545d200ea49f44bd236478175`  
Host: macOS arm64

## Baseline and attribution

Local `main` and `origin/main` both resolved to the baseline before the plan
bundle landed. The main worktree was clean. IMV0 began on
`codex/incremental-materialized-verification` after the plan bundle and the
proposed BLI plan were fast-forwarded into local `main`. IMV0 inherited no
pre-existing dirty file.

## Resolved Cargo graphs

Commands:

```bash
cargo tree -p nimbus-storage -e features
cargo tree -p nimbus-engine -e features
```

The storage-only graph resolves `serde_json` with `default`, `raw_value`, and
`std`. It does not resolve `preserve_order` or `indexmap`.

The shipped engine graph resolves `serde_json/preserve_order` and
`serde_json/indexmap`. The feature reaches the graph through
`workspace-hack`, `deno_core`, `deno_cache_dir`, `deno_graph`, `deno_npm`, and
`import_map`. Thus, one source digest has two map-order behaviors across the
two supported build graphs.

## Review-only probes

Command:

```bash
cargo test -p nimbus-engine --test imv0_review_probes -- --nocapture
```

The temporary test ran in the shipped graph. It produced these results:

```text
map_order forward=4af19810cad79c6b6cdb6412c53ec875bbf2317ebe344652c32931e51a9b8eda reversed=d27d9ccc6fdb7f8c8f0de56b5ac7212ca9fb97b00e451ccda063ad2c70176eaa
stored_value json=af8dcbd0a1ed913f74246a48a2d58887f3a40b0d03e976aa5255b63a50b0dc76 map=882ab4db8eb143b1160a3b635b4e25ffe044a9bea3f9c5da9ac2558757f02fa4
geopoint nan=cc0f88eb88d0d68b77e65799c0488449f4963cb8d11da28689048dd73274f7fc positive_infinity=cc0f88eb88d0d68b77e65799c0488449f4963cb8d11da28689048dd73274f7fc
test result: ok. 3 passed; 0 failed; 0 ignored
```

The probes prove three fail-before facts:

1. Reversing map insertion order changes the shipped-graph digest.
2. Equivalent `StoredValue::Json` and `StoredValue::Map` spellings change the
   digest although their JSON projections match.
3. NaN and positive-infinity GeoPoints produce the same digest.

The task deleted the review-only test file after capture. The file is absent
from the closing diff.

## Quick full-verifier measurement

Command:

```bash
cargo bench -p nimbus-engine --bench materialized-verification -- --quick --output docs/private/plans/proof/incremental-materialized-verification/imv0-raw.json
```

The retained JSON measures 10,000 documents with 1 KiB payloads and zero
churn. The successful run took 0.807184791 seconds wall time and 0.805851
seconds process CPU time. It made 2,438,248 allocations and allocated
1,693,703,373 bytes. Peak RSS was 307,445,760 bytes. The verifier reported
10,000 authoritative documents and no mismatch.

The current verifier has no byte-read counter. The JSON therefore records
`bytes_read` as `null` and labels that field `UNVERIFIED`. It does not invent a
measurement. IMV2 must add or independently measure that counter before the
continuation verdict.
