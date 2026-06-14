# NDS3 Cycle 87 - WebCrypto KMAC and usage canonicalization

Date: 2026-06-14

## Scope

Promoted six Node24 required-lane WebCrypto fixtures:

- `test/parallel/test-webcrypto-deduplicate-usages.js`
- `test/parallel/test-webcrypto-derivekey.js`
- `test/parallel/test-webcrypto-export-import.js`
- `test/parallel/test-webcrypto-keygen-kmac.js`
- `test/parallel/test-webcrypto-keygen.js`
- `test/parallel/test-webcrypto-sign-verify-kmac.js`

Fork tag: `nimbus/deno` `v2.8.3-nimbus.35`
(`d3e5ab6eff crypto: align WebCrypto KMAC parity`).

Nimbus pin: Deno-family crates in `Cargo.toml` / `Cargo.lock` repinned from
`v2.8.3-nimbus.34` to `v2.8.3-nimbus.35`.

`nimbus/rusty_v8` remained on `v149.4.0-nimbus.1`; no V8 or rusty_v8 code was
changed. No upstream Node fixture or checker was edited. No generated posture
JSON was hand-edited.

## Fork Fix

The Deno fork changed `ext/crypto/00_crypto.js`, `ext/crypto/key.rs`, and
`ext/crypto/lib.rs`.

The fork now:

- Adds WebCrypto algorithm registration for `KMAC128` and `KMAC256`.
- Implements KMAC sign/verify through cSHAKE with the NIST KMAC function-name
  domain separation, customization string, and output-length encoding.
- Supports KMAC `generateKey`, `importKey`, `exportKey`, `sign`, `verify`,
  `deriveKey`, `raw-secret`, and JWK surfaces used by Node24 fixtures.
- Canonicalizes CryptoKey usage intersection by the recognized usage list, which
  removes duplicate requested usages and returns Node-compatible usage order.
- Aligns ECDH too-short derived-bit OperationError text with Node.
- Copies generated RSA `algorithm.publicExponent` metadata instead of returning
  the caller's BufferSource by reference.

Fork-local checks:

```text
$ CARGO_NET_OFFLINE=true CARGO_ENCODED_RUSTFLAGS= cargo check -p deno_crypto
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.96s

$ cargo fmt --check -p deno_crypto
pass
```

## Dynamic Proof

Local fork override proof before publishing included the following focused
probes, all with `selected=1, passed=1, skipped=0, failed=0`:

```text
test/parallel/test-webcrypto-keygen-kmac.js        node24
test/parallel/test-webcrypto-sign-verify-kmac.js   node24
test/parallel/test-webcrypto-export-import.js      node24
test/parallel/test-webcrypto-deduplicate-usages.js node24
test/parallel/test-webcrypto-derivekey.js          node24
test/parallel/test-webcrypto-keygen.js             node24
test/parallel/test-webcrypto-sign-verify.js        node24 local-only
test/parallel/test-webcrypto-sign-verify.js        node22 local-only
```

After publishing `v2.8.3-nimbus.35`, removing the local Cargo path override, and
repinning Nimbus to the immutable tag, Cargo refreshed `Cargo.lock` offline and
the promotion guard was narrowed to the fixtures that stayed green on the
published tag:

```text
$ /opt/homebrew/bin/gtimeout -s KILL 240 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle87-webcrypto-tag-final \
  CARGO_NET_OFFLINE=true \
  cargo test -p nimbus-runtime --lib cycle87_webcrypto --locked -- --nocapture

node_compat node24-default-lane-executes-cycle87-webcrypto-batch node24 summary: selected=6, passed=6, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 898 filtered out; finished in 24.55s
```

Final `--locked` rerun after the offline lock refresh:

```text
$ /opt/homebrew/bin/gtimeout -s KILL 240 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle87-webcrypto-tag-final-locked \
  CARGO_NET_OFFLINE=true \
  cargo test -p nimbus-runtime --lib cycle87_webcrypto --locked -- --nocapture

node_compat node24-default-lane-executes-cycle87-webcrypto-batch node24 summary: selected=6, passed=6, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 897 filtered out; finished in 23.26s
```

## Not Promoted

`test/parallel/test-webcrypto-sign-verify.js` was deliberately left in the
required gap set even though it passed earlier local-path focused probes. On the
published tag it exceeded the harness wall-clock both inside the initial broad
cycle-87 batch and as a single-fixture probe:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
test/parallel/test-webcrypto-sign-verify.js: upstream node_compat fixture `test/parallel/test-webcrypto-sign-verify.js` exceeded wall-clock timeout
```

This avoids a false green. The fixture needs a further native-provider or
performance peel before promotion.

`test/parallel/test-webcrypto-sign-verify-eddsa.js` in Node22 still fails with
`NotSupportedError: Unrecognized algorithm name` and was not promoted.

## Regeneration And Checks

The classification/posture pipeline was regenerated with:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Generator checks:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --preserve-existing --check
node20.json is up to date
node22.json is up to date
node24.json is up to date
node26.json is up to date

$ /opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
node default support posture: pass

$ /opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
node required-surface blocker inventory: pass

$ cargo fmt --all --check
pass
```

Aggregate verifier:

```text
$ bash scripts/verify-node-default-runtime-support-hardening.sh
Summary: 13 passed, 21 failed
```

The remaining verifier failures are the expected open NDS closeout/gate
failures. Step 9 remains red because the regenerated posture is node22=10 /
node24=11, not 0/0.

## Diagnostics

Diagnostic roots retained:

- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle87-webcrypto-tag`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle87-webcrypto-tag-final`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle87-webcrypto-tag-final-locked`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle87-webcrypto-signverify-node24-tag-probe`

The failed early `nds3-cycle87-webcrypto-signverify-tag-probe` was caused by a
malformed extra-dir env value and produced no diagnostic artifact; it was not
used as proof.

## Regenerated Counts

After `classifications.py sync --lane all` and the standard status/dashboard/
trends/publish_evidence/default_support_posture/required_surface_blockers
regeneration pipeline:

```text
node22 gaps = 10, pass_rate_percent = 99.58
node24 gaps = 11, pass_rate_percent = 99.54
```

The gate remains red and honest; this cycle removes six Node24
V8-isolate-required gaps.
