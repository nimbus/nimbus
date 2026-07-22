# NDS3 Cycle 88 - WebCrypto EdDSA sign/verify

Date: 2026-06-14

## Scope

Promoted `test/parallel/test-webcrypto-sign-verify-eddsa.js` in the Node22
required lane.

Fork tag: `nimbus/deno` `v2.8.3-nimbus.36`
(`82e2afd177 crypto: add WebCrypto Ed448 sign/verify parity`).

Nimbus pin: Deno-family crates in `Cargo.toml` / `Cargo.lock` repinned from
`v2.8.3-nimbus.35` to `v2.8.3-nimbus.36`.

`nimbus/rusty_v8` remained on `v149.4.0-nimbus.1`; no V8 or rusty_v8 code was
changed. No upstream Node fixture or checker was edited. No generated posture
JSON was hand-edited.

## Fork Fix

The Deno fork changed `ext/crypto/00_crypto.js`, `ext/crypto/ed448.rs`, and
`ext/crypto/lib.rs`.

The fork now:

- Registers WebCrypto `Ed448` for `SubtleCrypto.sign()` and
  `SubtleCrypto.verify()`.
- Adds Ed448 raw sign/verify ops beside the existing Ed448 WebCrypto key
  import/export support, reusing the same `ed448_goldilocks` primitive shape as
  `deno_node_crypto`.
- Adds an EdDSA params dictionary for optional `context` and rejects non-empty
  Ed448 contexts with Node-compatible text.
- Aligns Ed25519/Ed448 wrong-key and wrong-algorithm sign/verify errors with
  Node's `Unable to use this key to sign/verify` expectations.

## Dynamic Proof

Local fork override proof before publishing, with Nimbus temporarily pointed at
`/Users/jack/src/github.com/nimbus/deno/ext/crypto`:

```text
$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle88-eddsa-local-7 \
  NIMBUS_RECENSUS_FIXTURE='test/parallel/test-webcrypto-sign-verify-eddsa.js' \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS='test/common:test/fixtures:test/fixtures/crypto:test/fixtures/keys' \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 898 filtered out; finished in 4.03s
```

Published-tag proof after pushing `v2.8.3-nimbus.36`, removing the local Cargo
path override, repinning `Cargo.toml` / `Cargo.lock`, and rebuilding from the
immutable tag:

```text
$ CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib nds_probe --locked --no-run

Compiling deno_crypto_provider v0.45.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.36#82e2afd1)
Compiling deno_crypto v0.265.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.36#82e2afd1)
Compiling nimbus-runtime v0.1.33
Finished `test` profile [unoptimized + debuginfo] target(s) in 41.29s

$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle88-eddsa-tag36-1 \
  NIMBUS_RECENSUS_FIXTURE='test/parallel/test-webcrypto-sign-verify-eddsa.js' \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS='test/common:test/fixtures:test/fixtures/crypto:test/fixtures/keys' \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 898 filtered out; finished in 4.04s
```

Promotion guard after deleting the scratch `nds_probe` file:

```text
$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle88-eddsa-promotion-tag36-2 \
  cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle88_webcrypto_eddsa_batch -- --nocapture

node_compat node22-supported-lane-executes-cycle88-webcrypto-eddsa-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 898 filtered out; finished in 3.81s
```

## Regeneration And Checks

The classification/posture pipeline was regenerated with:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
$ for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null; done
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

$ git diff --check
pass
```

## Diagnostics

Diagnostic roots retained:

- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle88-eddsa-local-6`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle88-eddsa-local-7`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle88-eddsa-tag36-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle88-eddsa-promotion-tag36-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle88-eddsa-promotion-tag36-2`

Earlier `nds3-cycle88-eddsa-local-5` failed because the scratch probe listed a
non-existent extra directory, `test/fixtures/webcrypto`. That was a probe setup
error, not a fixture failure, and was corrected before promotion.

## Regenerated Counts

After `classifications.py sync --lane all` and the standard status/dashboard/
trends/publish_evidence/default_support_posture/required_surface_blockers
regeneration pipeline:

```text
node22 gaps = 9, pass_rate_percent = 99.62
node24 gaps = 11, pass_rate_percent = 99.54
```

The gate remains red and honest; this cycle removes one Node22
V8-isolate-required gap.
