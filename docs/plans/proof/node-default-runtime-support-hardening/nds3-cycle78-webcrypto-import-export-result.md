# NDS3 cycle 78: WebCrypto EC/RSA import/export parity

Date: 2026-06-14

Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork pin: `v2.8.3-nimbus.27` (`ac1a64ad09876c0477afe039cc47104c47dab4ce`)  
rusty_v8 pin: `v149.4.0-nimbus.1`

## Fixtures

- `test/parallel/test-webcrypto-export-import-ec.js` (node22 + node24)
- `test/parallel/test-webcrypto-export-import-rsa.js` (node22 + node24)

Both fixtures exercise runtime-local WebCrypto import/export behavior. The
cycle stayed inside Deno fork crypto semantics and did not grant host process,
signal, subprocess, filesystem, or network authority.

## Fork Fix

Changed the Nimbus Deno fork WebCrypto implementation to align Node's EC/RSA
import/export validation and error text:

- unsupported requested usages now report `Unsupported key usage` where Node
  expects that SyntaxError wording, while DataError invalid-usage paths remain
  distinct;
- non-extractable key export uses Node's lowercase `key is not extractable`;
- empty import/generate usages use Node-compatible messages;
- EC JWK validation checks `crv`, `alg`, `use`, and missing `x`/`y` with Node
  fixture-compatible messages;
- RSA JWK validation checks `alg`, `use`, and missing public `n`/`e` with Node
  fixture-compatible messages;
- EC PKCS#8 import validates the EC public-key OID and private scalar shape.

Touched fork files:

- `ext/crypto/00_crypto.js`
- `ext/crypto/import_key.rs`

Published fork commit/tag:

```text
ac1a64ad09 crypto: align WebCrypto import/export errors
v2.8.3-nimbus.27
```

No official Node fixture or checker was edited. No generated posture JSON was
hand-edited.

## Dynamic Proof

Scratch `nds_probe` and temporary local Cargo path overrides were used only
while developing and removed before this checkpoint.

Local Deno path proof before tagging:

```text
node24 test/parallel/test-webcrypto-export-import-ec.js:
selected=1, passed=1, skipped=0, failed=0

node24 test/parallel/test-webcrypto-export-import-rsa.js:
selected=1, passed=1, skipped=0, failed=0

node22 test/parallel/test-webcrypto-export-import-ec.js:
selected=1, passed=1, skipped=0, failed=0

node22 test/parallel/test-webcrypto-export-import-rsa.js:
selected=1, passed=1, skipped=0, failed=0
```

Sibling exploratory probes that remain red and were not promoted:

- `test/parallel/test-webcrypto-export-import-cfrg.js`
- `test/parallel/test-webcrypto-export-import.js`
- `test/parallel/test-webcrypto-keygen.js`
- `test/parallel/test-crypto-key-objects-to-crypto-key.js`

Those failures are separate algorithm-support/HMAC/WebIDL work and are left in
the required-gap inventory.

## Repin Proof

After publishing `v2.8.3-nimbus.27`, Nimbus was repinned from
`v2.8.3-nimbus.26` to `v2.8.3-nimbus.27` in `Cargo.toml` and `Cargo.lock`. The
temporary local Deno path override was removed before rebuilding from the tag.

The immutable tag rebuild compiled `deno_crypto` from:

```text
https://github.com/nimbus/deno?tag=v2.8.3-nimbus.27#ac1a64ad
```

Repinned immutable-tag focused proof:

```text
tag27 node24 test-webcrypto-export-import-ec.js: selected=1, passed=1, skipped=0, failed=0
tag27 node24 test-webcrypto-export-import-rsa.js: selected=1, passed=1, skipped=0, failed=0
tag27 node22 test-webcrypto-export-import-ec.js: selected=1, passed=1, skipped=0, failed=0
tag27 node22 test-webcrypto-export-import-rsa.js: selected=1, passed=1, skipped=0, failed=0
```

## Promotion Guard

Added
`crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle78_webcrypto_import_export.rs`
and included it from `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`.

Final non-ignored promotion guard after removing the scratch probe:

```bash
cargo test -p nimbus-runtime --lib cycle78_webcrypto_import_export -- --nocapture
```

Result:

```text
node_compat node22-supported-lane-executes-cycle78-webcrypto-import-export-batch node22 summary: selected=2, passed=2, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle78-webcrypto-import-export-batch node24 summary: selected=2, passed=2, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 884 filtered out; finished in 12.01s
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
node22 v8_isolate_required.gaps = 17, pass_rate_percent = 99.28
node24 v8_isolate_required.gaps = 23, pass_rate_percent = 99.04
unique required fixtures across node22/node24 = 24
```

Before this cycle, the generated posture was:

```text
node22 v8_isolate_required.gaps = 19, pass_rate_percent = 99.2
node24 v8_isolate_required.gaps = 25, pass_rate_percent = 98.96
```

## Cleanup

- Removed scratch `nds_probe.rs` and its temporary `mod.rs` include.
- Removed the temporary local Deno Cargo path override before repinning.
- Restored unrelated Cargo.lock `itertools` dependency-selector churn introduced
  by the repin command; the final lockfile diff is only the Deno tag/hash repin.
- Verified `/Users/jack/src/github.com/nimbus/deno` is clean at the published
  tag:

```text
git status --short --branch
## nimbus/v2.8.3

git describe --tags --exact-match
v2.8.3-nimbus.27

git rev-parse --short=10 HEAD
ac1a64ad09
```

- Verified remote `nimbus/rusty_v8` default branch is `nimbus/v149.4.0`, and
  remote `README.md` on that branch already names `v149.4.0-nimbus.1` as the
  current Nimbus prebuilt release. No rusty_v8 README change was needed.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result: red, as expected. Summary was `13 passed, 21 failed`; step 9 still
fails because the regenerated posture is node22=17 / node24=23, not 0/0. The
remaining failures are the known private-plan/proof and closeout rows that
unblock only when the gate reaches literal green.
