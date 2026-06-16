# NDS3 cycle 43 - WebCrypto supports provider matrix

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-webcrypto-supports.mjs` was reclassified out of
`v8_isolate_required` as `upstream_or_platform_boundary` because its core
assertion is Node's exact native OpenSSL feature matrix, not a portable
in-isolate WebCrypto API guarantee.

Gate movement:

- node22: 58 gaps, 97.56% pass rate (unchanged)
- node24: 67 -> 66 gaps, 97.26% pass rate

No Deno fork tag was cut. Exploratory local edits to
`/Users/jack/src/github.com/nimbus/deno/ext/crypto/00_crypto.js` were reverted
after proving the fixture is blocked by native provider composition, not by the
supports overload shim alone.

## Source Evidence

The fixture imports `test/fixtures/webcrypto/supports-modern-algorithms.mjs`.
That support vector file derives expectations from Node's OpenSSL version:

- `pqc = hasOpenSSL(3, 5)`
- `ocb = hasOpenSSL(3)`
- `kmac = hasOpenSSL(3)`

Those booleans are then used to assert `SubtleCrypto.supports()` for AES-OCB,
KMAC128/256, ML-DSA, and ML-KEM. In Nimbus's Deno fork:

- `ext/node/polyfills/_process/process.ts` reports `process.versions.openssl`
  as `3.0.7+quic`.
- `ext/node/polyfills/process.ts` reports `process.features.openssl_is_boringssl`
  as `true`.
- `ext/crypto/00_crypto.js` registers AES-OCB and the ML-DSA / ML-KEM family in
  WebCrypto tables.
- `ext/crypto/mldsa.rs` and `ext/crypto/mlkem.rs` implement ML-DSA / ML-KEM
  through aws-lc / RustCrypto hooks.
- `ext/crypto/00_crypto.js` does not register KMAC128/256 WebCrypto algorithms,
  and there is no `ext/crypto/kmac.rs` implementation in the fork.

So the official fixture is not asking "does the isolate understand this
WebCrypto algorithm syntax?" It asks whether Nimbus exactly matches Node's
native OpenSSL build/dependency composition. Nimbus's aws-lc/BoringSSL-shaped
provider mix cannot satisfy that matrix honestly without either implementing the
KMAC native provider work or lying in `SubtleCrypto.supports()`.

## Focused Diagnostics

All focused probes used the scratch ignored `nds_probe` harness and were removed
before commit.

Local probe command shape:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-webcrypto-supports.mjs" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures/webcrypto" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'NDSDBG|summary: selected|should execute|test result|FAILED|AssertionError|ERR_ASSERTION|actual|expected|webcrypto-supports|supports|error\['
```

Observed sequence:

1. After the inherited exploratory `supports()` normalization patch, the fixture
   failed at AES-OCB nonce support:
   `selected=1, passed=0, skipped=0, failed=1`; error
   `expected true, got false`.
2. After a local AES-OCB supports-validator correction, AES-OCB vectors matched,
   and the fixture failed at ML-DSA:
   `selected=1, passed=0, skipped=0, failed=1`; error
   `expected false, got true`.
3. After a temporary diagnostic-only ML-DSA / ML-KEM false gate, the fixture
   failed at KMAC:
   `selected=1, passed=0, skipped=0, failed=1`; error
   `expected true, got false`.

The final diagnostic root was:

```text
/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat/diagnostics/general/node24__test_parallel_test_webcrypto_supports_mjs.json
```

## Verification

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Checks:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 134 entries
```

Generated counts:

```text
node22 58 97.56
node24 66 97.26
```

## Cleanup

- Removed the temporary `.cargo/config.toml` local path override.
- Removed the scratch `nds_probe` include and file.
- Reverted exploratory Deno fork edits; `/Users/jack/src/github.com/nimbus/deno`
  is clean on `nimbus/v2.8.3`.
- No V8/rusty_v8 files were edited.
