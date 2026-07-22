# NDS3 cycle 80: WebCrypto HMAC import/export parity

Date: 2026-06-14

Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork pin: `v2.8.3-nimbus.29` (`1a8b16bab430eed8bfdda00e343c2a2eb183aa9c`)  
rusty_v8 pin: `v149.4.0-nimbus.1`

## Fixture

- `test/parallel/test-webcrypto-export-import.js` (node22 only)

The node24 copy of the same fixture was not promoted in this cycle. It still
fails at the KMAC native-provider boundary:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: NotSupportedError: Unrecognized algorithm name
at test (.../test/parallel/test-webcrypto-export-import.js:173:30)
```

## Fork Fix

Changed the Nimbus Deno fork WebCrypto implementation to align the node22 HMAC
import/export path with Node:

- tags invalid `SubtleCrypto.importKey()` format conversion failures with
  `ERR_INVALID_ARG_VALUE`;
- tags non-JWK keyData conversion failures with `ERR_INVALID_ARG_TYPE`;
- tags missing HMAC `hash` normalization failures with `ERR_MISSING_OPTION`;
- reports `DataError: Invalid keyData` for `importKey("jwk", null, ...)`;
- aligns HMAC unsupported-usage, zero-length, too-short length, and invalid
  length error messages, preserving node24 wording by default and selecting the
  node22 wording under the existing Nimbus node-compat lane marker.

Touched fork file:

- `ext/crypto/00_crypto.js`

Published fork commit/tag:

```text
1a8b16bab4 crypto: align WebCrypto HMAC import errors
v2.8.3-nimbus.29
```

No official Node fixture or checker was edited. No generated posture JSON was
hand-edited.

## Local Path Proof

Nimbus was temporarily pointed at the local Deno fork package:

```text
paths = ["/Users/jack/src/github.com/nimbus/deno/ext/crypto"]
```

`deno_crypto` was narrowly rebuilt after each fork edit:

```bash
cargo clean -p deno_crypto
```

Focused node22 proof before tagging:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-webcrypto-export-import.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures:test/fixtures/crypto:test/fixtures/keys" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 888 filtered out; finished in 3.29s
```

## Repin Proof

After publishing `v2.8.3-nimbus.29`, Nimbus was repinned from
`v2.8.3-nimbus.28` to `v2.8.3-nimbus.29` in `Cargo.toml` and `Cargo.lock`. The
temporary local Deno path override was removed before rebuilding from the tag.

The immutable tag rebuild compiled the Deno-family crates from:

```text
https://github.com/nimbus/deno?tag=v2.8.3-nimbus.29#1a8b16ba
```

Focused node22 proof after repin:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 888 filtered out; finished in 3.71s
```

## Promotion Guard

Added
`crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle80_webcrypto_hmac_import_export.rs`
and included it from `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`.

Final non-ignored promotion guard:

```bash
CARGO_NET_OFFLINE=true /opt/homebrew/bin/gtimeout -s KILL 180 \
  cargo test -p nimbus-runtime --lib cycle80_webcrypto_hmac_import_export -- --nocapture
```

Result:

```text
node_compat node22-supported-lane-executes-cycle80-webcrypto-hmac-import-export-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 888 filtered out; finished in 3.72s
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
node22 v8_isolate_required.gaps = 15, pass_rate_percent = 99.37
node24 v8_isolate_required.gaps = 22, pass_rate_percent = 99.08
```

Before this cycle, the generated posture was:

```text
node22 v8_isolate_required.gaps = 16, pass_rate_percent = 99.32
node24 v8_isolate_required.gaps = 22, pass_rate_percent = 99.08
```

## Cleanup

- Removed scratch `nds_probe.rs` and its temporary `mod.rs` include.
- Removed the temporary local Deno Cargo path override before repinning.
- Verified `/Users/jack/src/github.com/nimbus/deno` is clean at the published
  tag:

```text
git status --short --branch
## nimbus/v2.8.3

git describe --tags --exact-match HEAD
v2.8.3-nimbus.29
```

## Verifier

The full verifier was not rerun for this cycle. The authoritative step-9 input
was regenerated and inspected directly: node22=15 / node24=22, so the gate
remains red and PR #10 stays draft.
