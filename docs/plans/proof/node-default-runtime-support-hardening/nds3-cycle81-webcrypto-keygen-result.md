# NDS3 cycle81 WebCrypto keygen result

Date: 2026-06-14

## Scope

Promote the node22 `test/parallel/test-webcrypto-keygen.js` fixture after a real
fork-owner fix in `nimbus/deno`. The node24 fixture is intentionally not
promoted: after the same fixes it still reaches KMAC native-provider support and
reports a dynamic failure.

## Fork changes

Fork: `/Users/jack/src/github.com/nimbus/deno`
Branch: `nimbus/v2.8.3`
Commit: `4bcf836240 crypto: align WebCrypto keygen parity`
Tag: `v2.8.3-nimbus.30`

Changed fork files:

- `ext/crypto/00_crypto.js`
- `ext/crypto/ed448.rs`
- `ext/crypto/generate_key.rs`
- `ext/crypto/lib.rs`
- `ext/node/polyfills/internal/crypto/util.ts`

The fork change adds Ed448 key generation, exposes
`bigIntArrayToUnsignedBigInt`, aligns RSA/AES `generateKey()` validation/error
codes, and updates the RSA publicExponent `OperationError` message.

## Dynamic proof

Local-path node22 focused probe after the fork fix:

```text
node_compat nds-probe node22 -> test/parallel/test-webcrypto-keygen.js
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 889 filtered out
```

Local-path node24 focused probe after the same fork fix:

```text
node_compat nds-probe node24 -> test/parallel/test-webcrypto-keygen.js
1..0 # Skipped: Skipping unsupported test cases
1..0 # Skipped: Skipping unsupported SHA-3 test case
1..0 # Skipped: Skipping unsupported AES-KW test cases
1..0 # Skipped: Skipping unsupported SHA-3 test case
1..0 # Skipped: Skipping unsupported Curve448 test cases
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: AssertionError [ERR_ASSERTION]:
  message: 'Unrecognized algorithm name'
at async test (.../test-webcrypto-keygen.js:250:5)
```

After tagging/pushing `v2.8.3-nimbus.30`, Nimbus was repinned from
`v2.8.3-nimbus.29` to `v2.8.3-nimbus.30`, the local Cargo path override was
removed, and `cargo update -p deno_crypto` locked the Deno-family crates to:

```text
git+https://github.com/nimbus/deno?tag=v2.8.3-nimbus.30#4bcf836240e151ad0fc0670c2754f332ee213697
```

Immutable-tag node22 focused proof:

```text
node_compat nds-probe node22 -> test/parallel/test-webcrypto-keygen.js
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 889 filtered out
```

Real non-ignored promotion guard:

```text
CARGO_NET_OFFLINE=true /opt/homebrew/bin/gtimeout -s KILL 180 \
  cargo test -p nimbus-runtime --lib cycle81_webcrypto_keygen -- --nocapture

node_compat node22-supported-lane-executes-cycle81-webcrypto-keygen-batch node22 -> test/parallel/test-webcrypto-keygen.js
node_compat node22-supported-lane-executes-cycle81-webcrypto-keygen-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 889 filtered out
```

## Regenerated posture

Regeneration command:

```text
PY=/opt/homebrew/bin/python3.12
$PY scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  $PY scripts/runtime/node/$s.py >/dev/null
done
```

Posture after regeneration:

```text
node22 14 99.41
node24 22 99.08
```

## Result

Honest promotion:

- node22 `test/parallel/test-webcrypto-keygen.js` leaves the
  `v8_isolate_required` gap set.
- node24 `test/parallel/test-webcrypto-keygen.js` stays red and remains in the
  required-surface blocker inventory because it reaches KMAC native-provider
  support.

No V8/rusty_v8 changes were made. No fixture or checker was edited. No derived
posture JSON was hand-edited.
