# DUA4 Dirty Work Reevaluation

status: done
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
source worktree: /Users/jack/src/github.com/nimbus/deno
source branch: nimbus/v2.8.1
source commit: 18f76a9a19ab74d49d9a40037733cc4aec983d26
pr: https://github.com/nimbus/nimbus/pull/11
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA4 is done. The Deno fork dirty/fresh
   compatibility reevaluation is committed and pushed as
   `18f76a9a19ab74d49d9a40037733cc4aec983d26`.
2. **Input baseline.** The row starts from the DUA3 rebased Deno candidate at
   `e65ddf9dc4a74b0adca7ef1d423dae47afa7caf7` plus the DUA1 hunk map.
3. **Disposition table.** Every DUA4 touched area is classified below.
4. **Implementation evidence.** Runtime and test/config changes are listed
   with owner and reason.
5. **Focused verification.** Focused Deno unit tests and official Node compat
   fixtures are recorded with exact pass/fail outcomes.
6. **Broad verification.** DUA4 does not promote broad compatibility counts;
   DUA6 owns broad reruns after immutable fork tags and Nimbus repin.
7. **Residual risks.** Diagnostic-only fixture failures are recorded as
   residual gaps and are not counted as positive compatibility claims.

## Input Baseline

| Field | Value |
| --- | --- |
| Deno upstream base | `denoland/deno@v2.8.1` |
| DUA3 candidate before this row | `e65ddf9dc4a74b0adca7ef1d423dae47afa7caf7` |
| DUA4 committed candidate | `18f76a9a19ab74d49d9a40037733cc4aec983d26` |
| Current rusty_v8 diagnostic pin | `v149.2.0-nimbus.1` |
| Closeout rusty_v8 tag | Not yet selected; DUA5 waits for hardened branch CI and `v149.2.0-nimbus.2` or later. |

DUA4 started with the Deno fork clean. The phrase "dirty work" in this row
refers to the current local compatibility patch stack classified by DUA1, not
uncommitted source changes at row start.

## Disposition Table

| Area | Files | Disposition | Evidence |
| --- | --- | --- | --- |
| CommonJS global path resolution | `ext/node/polyfills/01_require.js` | `still-needed-node-gap`, committed | `Module._findPath` no longer calls the Rust basename op on root-like search paths. This fixes `_preloadModules` and `runMain` regressions without restoring compile-cache code. |
| Deno worker constructors after locker seam | `runtime/web_worker.rs`, `runtime/worker.rs` | `nimbus-embedding-specific`, committed | Upstream Deno workers must explicitly use `use_locker: false`; Nimbus embedding can opt into locker mode through its own construction path. |
| Tokio metrics warning cleanup | `runtime/tokio_util.rs` | `nimbus-embedding-specific`, committed | Keeps the rebased fork warning-clean under normal builds while preserving `tokio_unstable` metrics behavior. |
| `node:v8` heap stats and queryObjects count behavior | `ext/node/polyfills/v8.ts`, `tests/unit_node/v8_test.ts` | `still-needed-node-gap`, committed | Node24 omits `total_allocated_bytes`, Node26 includes it. `queryObjects()` default now returns a count like Node; summary object listing remains residual because the current rusty_v8 binding does not expose live object handles. |
| `node:v8` serializer/deserializer | existing DUA3 V8 serializer code | `still-needed-node-gap`, carried | Focused Deno unit and official deserialize-buffer/version-tag fixtures pass. The official full serdes fixture remains diagnostic-only because the vendored Node26 fixture asserts a specific wire byte string. |
| Crypto random/cipher behavior | `tests/unit_node/crypto/crypto_cipher_test.ts`, `tests/unit_node/internal/_randomInt_test.ts`, `tests/node_compat/config.jsonc` | `still-needed-node-gap`, committed | Tests now match Node's async `randomInt` callback, unknown-cipher `ERR_CRYPTO_UNKNOWN_CIPHER` error shape, and ECB zero-IV behavior. The official cipher fixture is promoted from expected-fail to pass. |
| `internal_binding` js_stream additions | `tests/node_compat/config.jsonc` plus existing `internal_binding/js_stream.ts` DUA3 code | `still-needed-node-gap`, committed | Stale ignore for `test-js-stream-call-properties.js` removed; focused js_stream/wrap fixtures pass. |

No `module.enableCompileCache`, `flushCompileCache`, `getCompileCacheDir`, or
`compileCacheStatus` surface was reintroduced. A direct grep returned no
matches under `ext/node/polyfills/01_require.js` or `ext/node/polyfills/internal`.

## Implementation Evidence

Deno fork commit:

```console
git -C /Users/jack/src/github.com/nimbus/deno show --stat --oneline 18f76a9a19ab74d49d9a40037733cc4aec983d26
```

Observed:

```text
18f76a9a19 node: close focused DUA4 compatibility gaps
9 files changed, 39 insertions(+), 29 deletions(-)
```

Files changed:

- `ext/node/polyfills/01_require.js`
- `ext/node/polyfills/v8.ts`
- `runtime/tokio_util.rs`
- `runtime/web_worker.rs`
- `runtime/worker.rs`
- `tests/node_compat/config.jsonc`
- `tests/unit_node/crypto/crypto_cipher_test.ts`
- `tests/unit_node/internal/_randomInt_test.ts`
- `tests/unit_node/v8_test.ts`

The Deno branch was pushed:

```console
git -C /Users/jack/src/github.com/nimbus/deno push origin nimbus/v2.8.1
```

Observed: `e65ddf9dc4..18f76a9a19  nimbus/v2.8.1 -> nimbus/v2.8.1`.

## Focused Verification

Formatting and build:

```console
/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x fmt
/usr/bin/env CARGO_ENCODED_RUSTFLAGS= cargo build --bin deno --bin test_server
git -C /Users/jack/src/github.com/nimbus/deno diff --check
```

Observed:

- `./x fmt`: `Formatting complete.`
- `cargo build --bin deno --bin test_server`: `Finished dev profile ... in 25.78s`.
- `git diff --check`: passed with no output.

Focused Deno unit probes:

| Command | Result |
| --- | --- |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-node module_test` | `1 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-node v8_test` | `1 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-node crypto_cipher_test` | `1 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-node _random` | `3 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-node crypto_misc_test` | `1 tests passed` |

Focused official Node compatibility probes:

| Command | Result |
| --- | --- |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-compat js-stream` | `6 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-compat test-crypto-cipheriv-decipheriv` | `1 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-compat test-crypto-random` | `3 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-compat test-crypto-getcipherinfo` | `1 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-compat test-v8-version-tag` | `1 tests passed` |
| `/usr/bin/env CARGO_ENCODED_RUSTFLAGS= ./x test-compat test-v8-deserialize-buffer` | `1 tests passed` |

Diagnostic-only probes, not counted as positive compatibility claims:

| Command | Outcome | Disposition |
| --- | --- | --- |
| `./x test-compat test-module-loading-globalpaths` | failed because the fixture copies `deno` to a child path and invokes it without permissions, so the child cannot read the fixture directory before testing global paths | DUA6/NDS must adapt the fixture or lane semantics before using it as a pass/fail signal. |
| `./x test-compat test-v8-stats` | failed because the vendored Node26 fixture expects `total_allocated_bytes` while the current default Deno process advertises Node `24.2.0` | DUA6/NDS must run version-matched fixture lanes instead of treating Node26 expectations as Node24 default expectations. |
| `./x test-compat test-v8-serdes` | failed on an exact serialized byte-string fixture (`ff106f...` vs expected `ff0f6f...`) | This is a V8-wire-format fixture and must be resolved under the versioned official fixture lane before promotion. |
| `./x test-compat test-v8-query-objects` | improved past default count, then failed on `format: "summary"` object listing | Count behavior is fixed; live-object summary remains a real Node API gap. |

## Broad Verification

DUA4 intentionally did not run or update broad Node compatibility counts. DUA6
owns the broad before/after rerun after DUA5 publishes immutable Deno and
`rusty_v8` tags and Nimbus repins to them. Newly green fixtures from this row
are therefore not promoted beyond focused proof.

## Residual Risks

- The Deno fork still points at diagnostic `v149.2.0-nimbus.1`. DUA5 must wait
  for the hardened `rusty_v8` branch CI and a superseding tag before Nimbus
  repin.
- `v8.queryObjects(..., { format: "summary" })` does not return Node-style
  live object summaries. This is a real remaining compatibility gap, not a
  test issue.
- The official global-path fixture needs a Deno runner adaptation because its
  copied child executable is invoked without Deno permissions.
- The official Node26 V8 stats/serdes fixtures must be routed through
  version-matched lanes; DUA4 does not claim those as green.
