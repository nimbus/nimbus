# NDS3 cycle 79: WebCrypto CFRG import/export parity

Date: 2026-06-14

Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork pin: `v2.8.3-nimbus.28` (`c1de56b61484f0d258847f3f4309228dfd3659a0`)  
rusty_v8 pin: `v149.4.0-nimbus.1`

## Fixtures

- `test/parallel/test-webcrypto-export-import-cfrg.js` (node22 + node24)

This fixture exercises runtime-local WebCrypto CFRG key import/export behavior
for Ed25519, Ed448, X25519, and X448. The cycle stayed inside Deno fork crypto
semantics and did not grant host process, signal, subprocess, filesystem, or
network authority.

## Fork Fix

Changed the Nimbus Deno fork WebCrypto implementation to align Node's CFRG
import/export behavior:

- added Ed448 SPKI, PKCS#8, raw public, and JWK import/export support for
  `SubtleCrypto.importKey`, `SubtleCrypto.exportKey`, and private-key
  public-key derivation;
- aligned Ed25519, X25519, and X448 JWK/DER validation messages with the Node
  fixture's expected DataError/SyntaxError text;
- fixed X448 public-key derivation and deriveBits to use RFC 7748 raw clamped
  scalar bits instead of reducing the scalar through Ed448 group arithmetic;
- added a Deno fork unit vector proving the X448 helper against the Node CFRG
  fixture's private/public JWK pair.

Touched fork files:

- `Cargo.toml`
- `Cargo.lock`
- `ext/crypto/Cargo.toml`
- `ext/crypto/00_crypto.js`
- `ext/crypto/ed448.rs`
- `ext/crypto/lib.rs`
- `ext/crypto/x448.rs`

Published fork commit/tag:

```text
c1de56b614 crypto: support WebCrypto CFRG import export
v2.8.3-nimbus.28
```

No official Node fixture or checker was edited. No generated posture JSON was
hand-edited.

## Dynamic Proof

Scratch `nds_probe` and a temporary local Cargo path override were used only
while developing and removed before this checkpoint.

Deno fork helper proof:

```bash
CARGO_ENCODED_RUSTFLAGS='' cargo test -p deno_crypto \
  x448_scalar_mult_matches_node_cfrg_vector -- --nocapture
```

Result:

```text
test x448::tests::x448_scalar_mult_matches_node_cfrg_vector ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 1.02s
```

Local Deno path proof before tagging:

```text
node22 test/parallel/test-webcrypto-export-import-cfrg.js:
selected=1, passed=1, skipped=0, failed=0

node24 test/parallel/test-webcrypto-export-import-cfrg.js:
selected=1, passed=1, skipped=0, failed=0
```

## Repin Proof

After publishing `v2.8.3-nimbus.28`, Nimbus was repinned from
`v2.8.3-nimbus.27` to `v2.8.3-nimbus.28` in `Cargo.toml` and `Cargo.lock`. The
temporary local Deno path override was removed before rebuilding from the tag.

The immutable tag rebuild compiled `deno_crypto` from:

```text
https://github.com/nimbus/deno?tag=v2.8.3-nimbus.28#c1de56b6
```

Repinned immutable-tag focused proof:

```text
tag28 node22 test-webcrypto-export-import-cfrg.js: selected=1, passed=1, skipped=0, failed=0
tag28 node24 test-webcrypto-export-import-cfrg.js: selected=1, passed=1, skipped=0, failed=0
```

## Promotion Guard

Added
`crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle79_webcrypto_cfrg_import_export.rs`
and included it from `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`.

Final non-ignored promotion guard after removing the scratch probe:

```bash
CARGO_NET_OFFLINE=true /opt/homebrew/bin/gtimeout -s KILL 240 \
  cargo test -p nimbus-runtime --lib cycle79_webcrypto_cfrg_import_export -- --nocapture
```

Result:

```text
node_compat node22-supported-lane-executes-cycle79-webcrypto-cfrg-import-export-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle79-webcrypto-cfrg-import-export-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 886 filtered out; finished in 7.66s
```

## Regeneration

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Generated posture after regeneration:

```text
node22 v8_isolate_required.gaps = 16, pass_rate_percent = 99.32
node24 v8_isolate_required.gaps = 22, pass_rate_percent = 99.08
```

Before this cycle, the generated posture was:

```text
node22 v8_isolate_required.gaps = 17, pass_rate_percent = 99.28
node24 v8_isolate_required.gaps = 23, pass_rate_percent = 99.04
```

## Cleanup

- Removed scratch `nds_probe.rs` and its temporary `mod.rs` include.
- Removed the temporary local Deno Cargo path override before repinning.
- Verified `/Users/jack/src/github.com/nimbus/deno` is clean at the published
  tag:

```text
git status --short --branch
## nimbus/v2.8.3

git describe --tags --exact-match
v2.8.3-nimbus.28

git rev-parse HEAD
c1de56b61484f0d258847f3f4309228dfd3659a0
```

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result: red, as expected. Summary was `13 passed, 21 failed` in this local
checkout; step 9 still fails because the regenerated posture is node22=16 /
node24=22, not 0/0. Several other failures are private-plan/proof and closeout
rows that remain unresolved until the gate reaches literal green.
