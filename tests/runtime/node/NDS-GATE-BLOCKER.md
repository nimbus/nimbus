# NDS gate — FORMAL documented blocked state (cycle-26, 2026-06-12)

**Branch/PR:** worktree node-default-runtime-support-hardening -> PR #10  
**Fork:** nimbus/deno v2.8.2-nimbus.33 (f4994a93); nimbus/rusty_v8 stock v149.2.0-nimbus.1  
**Verifier:** `bash scripts/verify-node-default-runtime-support-hardening.sh` step 9

## Unsatisfied gate

Step 9 needs both lanes `gaps==0` AND `pass_rate==100`. Committed posture: **node22 = 73**, **node24 = 81**. Not 0/0.

Session cycles 17-26 reduced the gate 81/87 -> 73/81 by harvesting every cheap/clean/
TS-tractable lever (published fork fixes hasAsyncGraph, createCachedData, the
SourceTextModule error-semantics parity set, and AbortController/AbortSignal
inspect + timeout reachability; source-confirmed host-process reclassification
for aborted-util; promotions; all dynamically green-guarded or structurally
source-confirmed, zero false greens, regression-verified). 85 unique fixtures
remain (node22=73, node24=81).

## Genuinely blocked (cannot be reached in the V8-isolate/runtime/fork scope on this host)

- `test/parallel/test-vm-module-hastoplevelawait.js` (24) — owner: nimbus/rusty_v8 (Module::HasTopLevelAwait binding -> from-source V8 -> OOM)
- `test/parallel/test-vm-module-import-meta.js` (22+24) — owner: nimbus/deno (libs/core/runtime/bindings.rs:1104 + ext/node vm initializeImportMeta wiring)
- `test/parallel/test-webcrypto-keygen-kmac.js` (24) — owner: nimbus/deno (ext/crypto: Ed448/KMAC native primitives; may be absent from aws-lc)
- `test/parallel/test-webcrypto-sign-verify-eddsa.js` (22) — owner: nimbus/deno (ext/crypto: Ed448/KMAC native primitives; may be absent from aws-lc)
- `test/parallel/test-webcrypto-sign-verify-kmac.js` (24) — owner: nimbus/deno (ext/crypto: Ed448/KMAC native primitives; may be absent from aws-lc)

## Tractable but deep (sustained multi-session native deno_core/deno_crypto work — task #61)

### DEEP_behavioral_misc (16) — owner: nimbus/deno (per-fixture deno_node/deno_core)
- `test/async-hooks/test-httpparser-reuse.js` (22+24)
- `test/parallel/test-assert-calltracker-calls.js` (22+24)
- `test/parallel/test-assert-deep.js` (22+24)
- `test/parallel/test-assert.js` (22)
- `test/parallel/test-console.js` (22)
- `test/parallel/test-error-prepare-stack-trace.js` (22+24)
- `test/parallel/test-events-uncaught-exception-stack.js` (22+24)
- `test/parallel/test-file-write-stream5.js` (22+24)
- `test/parallel/test-global.js` (22+24)
- `test/parallel/test-process-get-builtin.mjs` (22+24)
- `test/parallel/test-source-map-invalid-url.js` (22+24)
- `test/parallel/test-stream-readable-compose.js` (24)
- `test/parallel/test-stream-writable-samecb-singletick.js` (22+24)
- `test/parallel/test-structuredClone-global.js` (22+24)
- `test/parallel/test-util-styletext.js` (22+24)
- `test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js` (22+24)

### DEEP_crypto_provider (15) — owner: nimbus/deno (ext/crypto / deno_node_crypto)
- `test/parallel/test-crypto-authenticated.js` (22+24)
- `test/parallel/test-crypto-des3-wrap.js` (22+24)
- `test/parallel/test-webcrypto-deduplicate-usages.js` (24)
- `test/parallel/test-webcrypto-derivebits-hkdf.js` (22+24)
- `test/parallel/test-webcrypto-derivekey.js` (24)
- `test/parallel/test-webcrypto-encrypt-decrypt-aes.js` (24)
- `test/parallel/test-webcrypto-export-import-cfrg.js` (22+24)
- `test/parallel/test-webcrypto-export-import-ec.js` (22+24)
- `test/parallel/test-webcrypto-export-import-rsa.js` (22+24)
- `test/parallel/test-webcrypto-export-import.js` (22+24)
- `test/parallel/test-webcrypto-keygen.js` (22+24)
- `test/parallel/test-webcrypto-promise-prototype-pollution.mjs` (24)
- `test/parallel/test-webcrypto-sign-verify.js` (22+24)
- `test/parallel/test-webcrypto-supports.mjs` (24)
- `test/parallel/test-webcrypto-wrap-unwrap.js` (22+24)

### DEEP_domain_regression_risk (3) — owner: nimbus/deno (ext/node domain.ts — regression-risky)
- `test/parallel/test-domain-async-id-map-leak.js` (22+24)
- `test/parallel/test-domain-set-uncaught-exception-capture-after-load.js` (22+24)
- `test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js` (22+24)

### DEEP_esm_loader (10) — owner: nimbus/deno (deno_core module loader)
- `test/es-module/test-esm-cjs-named-error.mjs` (22+24)
- `test/es-module/test-esm-dynamic-import-commonjs.js` (22+24)
- `test/es-module/test-esm-dynamic-import-commonjs.mjs` (22+24)
- `test/es-module/test-esm-dynamic-import.js` (22+24)
- `test/es-module/test-esm-loader-mock.mjs` (22+24)
- `test/es-module/test-esm-require-race-condition.js` (24)
- `test/es-module/test-esm-snapshot.mjs` (22+24)
- `test/es-module/test-esm-virtual-json.mjs` (22+24)
- `test/parallel/test-performance-many-marks.js` (22+24)
- `test/parallel/test-v8-serialize-leak.js` (22+24)

### DEEP_eventloop_timers (6) — owner: nimbus/deno (deno_core event loop)
- `test/parallel/test-process-beforeexit.js` (22+24)
- `test/parallel/test-timers-immediate-queue-throw.js` (22+24)
- `test/parallel/test-timers-immediate-unref-nested-once.js` (22+24)
- `test/parallel/test-timers-immediate-unref-simple.js` (22+24)
- `test/parallel/test-timers-immediate-unref.js` (22+24)
- `test/parallel/test-timers-reset-process-domain-on-throw.js` (22+24)

### DEEP_fs_sandbox (10) — owner: nimbus/deno (deno_fs) + nimbus-runtime sandbox
- `test/async-hooks/test-tlswrap.js` (22+24)
- `test/es-module/test-wasm-web-api.js` (22+24)
- `test/parallel/test-fs-promises.js` (22+24)
- `test/parallel/test-fs-read-stream.js` (22+24)
- `test/parallel/test-fs-realpath.js` (22+24)
- `test/parallel/test-fs-stat-date.mjs` (22+24)
- `test/parallel/test-fs-symlink-dir-junction-relative.js` (22+24)
- `test/parallel/test-fs-symlink-dir-junction.js` (22+24)
- `test/parallel/test-fs-symlink.js` (24)
- `test/parallel/test-fs-utimes-y2K38.js` (22+24)

### DEEP_hang_timeout (5) — owner: nimbus/deno + nimbus-runtime
- `test/parallel/test-gc-http-client-connaborted.js` (22+24)
- `test/parallel/test-perf-hooks-eventlooputilization.js` (24)
- `test/parallel/test-performance-eventlooputil.js` (22)
- `test/parallel/test-stream-base-typechecking.js` (22+24)
- `test/parallel/test-webstreams-clone-unref.js` (22+24)

### DEEP_promise_hooks (6) — owner: nimbus/deno (deno_core promise hooks)
- `test/parallel/test-heapdump-async-hooks-init-promise.js` (22+24)
- `test/parallel/test-promise-hook-create-hook.js` (22+24)
- `test/parallel/test-promise-hook-exceptions.js` (22+24)
- `test/parallel/test-promise-hook-on-after.js` (22+24)
- `test/parallel/test-promise-hook-on-resolve.js` (22+24)
- `test/parallel/test-promise-swallowed-event.js` (22+24)

### DEEP_vm_semantics (9) — owner: nimbus/deno (ext/node/ops/vm.rs + deno_core)
- `test/parallel/test-vm-dynamic-import-callback-missing-flag.js` (22+24)
- `test/parallel/test-vm-global-property-interceptors.js` (22+24)
- `test/parallel/test-vm-global-property-prototype.js` (22+24)
- `test/parallel/test-vm-module-after-evaluate.js` (22+24)
- `test/parallel/test-vm-module-basic.js` (22+24)
- `test/parallel/test-vm-module-dynamic-import.js` (22+24)
- `test/parallel/test-vm-module-referrer-realm.mjs` (22+24)
- `test/parallel/test-vm-script-after-evaluate.js` (22+24)
- `test/parallel/test-vm-timeout-escape-promise-module.js` (22+24)

## Conclusion

0/0 is not reachable within a single session. The genuinely-blocked subset is a true
blocker (rusty_v8 OOM binding, deno_core import-meta panic needing cross-boundary
initializeImportMeta wiring, native Ed448/KMAC primitives maybe absent from aws-lc). The
DEEP categories are individually tractable via the proven fork-owner flow but constitute a
multi-session effort. Gate held RED and honest at node22=73 / node24=81.
