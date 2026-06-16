# NDS3 cycle 34 result - assert deep comparison parity

Date: 2026-06-13

## Scope

Fixed and promoted the required fixture on both default lanes:

- `test/parallel/test-assert-deep.js` (node22, node24)

Deno fork release:

- `nimbus/deno` commit `6cef6731e5`
- tag `v2.8.3-nimbus.4`

Fork changes:

- `ext/crypto/00_crypto.js` now initializes `internals.kKeyObject` when WebCrypto
  loads before Node crypto constants, so WebCrypto `CryptoKey` instances share
  the key-material symbol used by Node comparison helpers.
- `ext/node/polyfills/internal/util/comparisons.ts` now compares `CryptoKey.type`
  and uses the existing Nimbus node-compat lane marker to preserve the older
  Node20/Node22 circular comparison behavior while keeping Node24 behavior by
  default.

## Proof

Local-fork focused census after a temporary Cargo path override to
`/Users/jack/src/github.com/nimbus/deno/ext/crypto` and
`/Users/jack/src/github.com/nimbus/deno/ext/node`:

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-assert-deep.js" \
  NIMBUS_RECENSUS_LANE=node22 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|deep-equal|Missing expected|AssertionError|at .*test-assert-deep|\+ actual|- expected|Crypto|Circular'
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 819 filtered out
```

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-assert-deep.js" \
  NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|deep-equal|Missing expected|AssertionError|at .*test-assert-deep|\+ actual|- expected|Crypto|Circular'
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 819 filtered out
```

Immutable-tag focused census after publishing `v2.8.3-nimbus.4`, removing the
Cargo path override, repinning `Cargo.toml`/`Cargo.lock`, and cleaning only
`deno_crypto` and `deno_node`:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 819 filtered out

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 819 filtered out
```

Promoted non-ignored guards:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib cycle34_assert_deep -- --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|test runtime::tests::node_compat::node|passed=|skipped=|failed='
```

```text
node_compat node22-supported-lane-executes-cycle34-assert-deep-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test runtime::tests::node_compat::node22_supported_lane_executes_cycle34_assert_deep_batch ... ok
node_compat node24-default-lane-executes-cycle34-assert-deep-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test runtime::tests::node_compat::node24_default_lane_executes_cycle34_assert_deep_batch ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 820 filtered out
```

Generated posture after classification sync and evidence regeneration:

```text
node22: v8_isolate_required.gaps = 65, pass_rate_percent = 97.26
node24: v8_isolate_required.gaps = 75, pass_rate_percent = 96.89
unique required fixtures remaining: 77
```

## Guardrails

- No V8 or rusty_v8 changes.
- No official fixture or checker edits.
- No hand-edited false-green JSON.
- Temporary Cargo path override was removed before immutable-tag proof.
- Scratch `nds_probe` include/file was removed before promotion.
- PR #10 remains draft; the gate is still red and honest.
