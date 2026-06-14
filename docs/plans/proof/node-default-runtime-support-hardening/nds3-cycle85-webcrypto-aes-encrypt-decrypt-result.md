# NDS3 Cycle 85 - Node24 WebCrypto AES encrypt/decrypt

Date: 2026-06-14

## Scope

Promoted `test/parallel/test-webcrypto-encrypt-decrypt-aes.js` in the Node24
required lane only.

Fork tag: `nimbus/deno` `v2.8.3-nimbus.34`
(`1dfa4fd884 crypto: align WebCrypto AES parity`).

Nimbus pin: Deno-family crates in `Cargo.toml` / `Cargo.lock` repinned from
`v2.8.3-nimbus.33` to `v2.8.3-nimbus.34`.

`nimbus/rusty_v8` remained on `v149.4.0-nimbus.1`; no V8 or rusty_v8 code was
changed. The `nimbus/rusty_v8` remote default branch was checked with
`git remote show origin` and is `nimbus/v149.4.0`. The default-branch README
already names the Deno `v2.8.3` / rusty_v8 `v149.4.0-nimbus.1` foundation and
uses `v149.4.0-nimbus.1` in the mirror examples. The already-published
`v149.4.0-nimbus.1` tag was not moved.

## Fork Fix

`ext/crypto/00_crypto.js` now matches Node24's AES encrypt/decrypt fixture
expectations for:

- AES encrypt/decrypt key-algorithm mismatch and key-usage DOMException names
  and messages, while preserving Node22 lane messages.
- AES-CBC/AES-CTR invalid IV/counter messages in the Node24 lane.
- AES-GCM/AES-OCB invalid tag-length messages.
- AES-GCM variable-length nonce handling, rejecting only empty IVs at this
  validation layer instead of requiring 12 or 16 bytes.

The fork patch was intentionally narrowed before tagging. Earlier local
diagnostics explored WebCrypto usage deduplication, sign/verify message parity,
and AEAD `Decipheriv.final()` error ordering, but those did not produce a full
fixture promotion in this wave and were not included in `v2.8.3-nimbus.34`.

## Dynamic Proof

Local fork override proof before publishing:

```text
$ /opt/homebrew/bin/gtimeout -s KILL 240 env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-aes-only-local CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib nds_probe_webcrypto_node24_aes --offline -- --ignored --nocapture
node_compat nds-probe-webcrypto-node24-aes node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 900 filtered out; finished in 3.26s
```

Published-tag proof after removing the local Cargo path override and repinning
to `v2.8.3-nimbus.34`:

```text
$ /opt/homebrew/bin/gtimeout -s KILL 240 env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-aes-tag CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib nds_probe_webcrypto_node24_aes --locked -- --ignored --nocapture
node_compat nds-probe-webcrypto-node24-aes node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 900 filtered out; finished in 3.08s
```

Promotion guard:

```text
$ /opt/homebrew/bin/gtimeout -s KILL 240 env CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib cycle85_webcrypto_aes_encrypt_decrypt --locked -- --nocapture
node_compat node24-default-lane-executes-cycle85-webcrypto-aes-encrypt-decrypt-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 896 filtered out; finished in 3.00s
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
```

`publish_docs.py` was attempted after posture/blocker regeneration, but this
worktree does not have the untracked public shim inventory input it requires:
`docs/architecture/runtime/node-isolate-shim-inventory.json`. The gate-critical
classification, posture, blocker, and latest evidence generators were still
refreshed and validated; no generated JSON was hand-edited.

## Diagnostics

Diagnostic roots retained:

- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-crypto-probe`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-crypto-local`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-targeted-local`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-focused-local`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-focused-local-final`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-focused-local-final-rerun`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-aes-only-local`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle85-webcrypto-aes-tag`

Broad/targeted diagnostics found but did not promote:

- `test/parallel/test-crypto-authenticated.js` advanced past the original
  `Cannot change encoding` AEAD error-ordering failure under an exploratory
  `Decipheriv.final()` patch, but then stopped on missing CCM support
  (`Unknown cipher aes-128-ccm`). That patch was not tagged.
- `test/parallel/test-webcrypto-deduplicate-usages.js` advanced past HMAC usage
  duplication under an exploratory usage-order patch, but then stopped on KMAC
  algorithm support. That patch was not tagged.
- `test/parallel/test-webcrypto-sign-verify.js` in Node22 passed earlier in the
  diagnostic flow but timed out twice during final pre-narrowing reruns, so it
  was not promoted.

## Regenerated Counts

After `classifications.py sync --lane all` and the standard status/dashboard/
trends/publish_evidence/default_support_posture/required_surface_blockers
regeneration pipeline:

```text
node22 gaps = 11, pass_rate_percent = 99.53
node24 gaps = 18, pass_rate_percent = 99.25
```

The gate remains red and honest; this cycle removes one Node24
V8-isolate-required gap.
