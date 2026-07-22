# NDS3 Cycle 89 - Authenticated crypto cipher parity

Date: 2026-06-14

## Scope

Promoted `test/parallel/test-crypto-authenticated.js` in both required lanes:
Node22 and Node24.

Fork tag: `nimbus/deno` `v2.8.3-nimbus.37`
(`e909e14ea7 node: add AES-CCM authenticated cipher parity`).

Nimbus pin: Deno-family crates in `Cargo.toml` / `Cargo.lock` repinned from
`v2.8.3-nimbus.36` to `v2.8.3-nimbus.37`.

`nimbus/rusty_v8` remained on `v149.4.0-nimbus.1`; no V8 or rusty_v8 code was
changed. No upstream Node fixture or checker was edited. No generated posture
JSON was hand-edited.

## Fork Fix

The Deno fork changed:

- `ext/node/polyfills/internal/crypto/cipher.ts`
- `ext/node/polyfills/internal/crypto/util.ts`
- `ext/node_crypto/cipher.rs`
- `ext/node_crypto/lib.rs`

The fork now:

- Advertises and implements Node's AES-CCM cipher names
  (`aes-128-ccm`, `aes-192-ccm`, `aes-256-ccm`).
- Implements AES-CCM authenticated encryption/decryption in
  `deno_node_crypto`, including CCM nonce/tag validation, AAD handling,
  tag generation, and tag verification.
- Preserves the native crypto failure before lazy output decoder creation, so
  auth failures are not masked by a later encoding-state error.
- Adds Node-style `ERR_CRYPTO_INVALID_AUTH_TAG` metadata for invalid
  authenticated-cipher tag lengths.
- Normalizes `DataView` inputs in cipher/decipher `update()` before calling
  the native op, matching the Node24 authenticated-crypto regression block.

## Dynamic Proof

Local fork override proof before publishing, with Nimbus temporarily pointed at
`/Users/jack/src/github.com/nimbus/deno/ext/node` and
`/Users/jack/src/github.com/nimbus/deno/ext/node_crypto`:

```text
$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node22-local-4 \
  NIMBUS_RECENSUS_FIXTURE=test/parallel/test-crypto-authenticated.js \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 899 filtered out; finished in 2.77s

$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node24-local-2 \
  NIMBUS_RECENSUS_FIXTURE=test/parallel/test-crypto-authenticated.js \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 899 filtered out; finished in 2.93s
```

Published-tag proof after pushing `v2.8.3-nimbus.37`, removing the local Cargo
path override, repinning `Cargo.toml` / `Cargo.lock`, and rebuilding from the
immutable tag:

```text
$ CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib nds_probe --no-run

Compiling deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.37#e909e14e)
Compiling deno_node_crypto v0.21.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.37#e909e14e)
Compiling deno_node_sqlite v0.21.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.37#e909e14e)
Finished `test` profile [unoptimized + debuginfo] target(s) in 40.32s

$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node22-tag37-1 \
  NIMBUS_RECENSUS_FIXTURE=test/parallel/test-crypto-authenticated.js \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 899 filtered out; finished in 2.98s

$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node24-tag37-1 \
  NIMBUS_RECENSUS_FIXTURE=test/parallel/test-crypto-authenticated.js \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 899 filtered out; finished in 2.99s
```

Promotion guard after deleting the scratch `nds_probe` file:

```text
$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-promotion-tag37-1 \
  cargo test -p nimbus-runtime --lib cycle89_crypto_authenticated -- --nocapture

node_compat node22-supported-lane-executes-cycle89-crypto-authenticated-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle89-crypto-authenticated-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 899 filtered out; finished in 5.42s
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

- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node22-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node22-local-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node22-local-2`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node22-local-3`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node22-local-4`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node24-local-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node24-local-2`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node22-tag37-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-node24-tag37-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle89-crypto-auth-promotion-tag37-1`

Earlier roots captured the normal peel:

- `.36` initially masked an auth failure behind `Cannot change encoding`.
- After decoder-order parity, the fixture advanced to missing
  `aes-128-ccm`.
- After AES-CCM support, node22 advanced to missing
  `ERR_CRYPTO_INVALID_AUTH_TAG` metadata.
- Node24 then exposed the later zero-length `DataView` CCM regression block.

Each peel was fixed before promotion; only the published-tag and promotion roots
are used as green proof.

## Regenerated Counts

After `classifications.py sync --lane all` and the standard status/dashboard/
trends/publish_evidence/default_support_posture/required_surface_blockers
regeneration pipeline:

```text
node22 gaps = 8, pass_rate_percent = 99.66
node24 gaps = 10, pass_rate_percent = 99.58
```

The gate remains red and honest; this cycle removes one Node22 and one Node24
V8-isolate-required gap.
