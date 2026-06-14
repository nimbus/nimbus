# NDS gate - FORMAL documented blocked state (cycle-70, 2026-06-14)

**Branch/PR:** worktree node-default-runtime-support-hardening -> PR #10  
**Fork:** nimbus/deno v2.8.3-nimbus.22 (7c0004b48e); nimbus/rusty_v8 stock v149.4.0-nimbus.1  
**Verifier:** `bash scripts/verify-node-default-runtime-support-hardening.sh` step 9

## Unsatisfied gate

Step 9 needs both lanes `gaps==0` AND `pass_rate==100`. Current generated posture: **node22 = 25**, **node24 = 32**. Not 0/0.

Session cycles 17-70 reduced the gate 81/87 -> 25/32 by harvesting every cheap/clean/
TS-tractable lever (published fork fixes hasAsyncGraph, createCachedData, the
SourceTextModule error-semantics parity set, and AbortController/AbortSignal
inspect + timeout reachability; source-confirmed host-process reclassification
for aborted-util; structuredClone option errors, Blob transfer rejection, and
MessagePort unref parity; source-confirmed host-TTY reclassification for
util.styleText stream validation; Deno/rusty_v8 v2.8.3/v149.4.0 foundation bump;
process.getBuiltinModule identity; tolerant data-url base64url decoding for invalid
source-map URL tolerance; console symbol-property inspect parity; assert calltracker
calls promotion; assert promotion; assert-deep cycle/CryptoKey parity; default
Error.prepareStackTrace plus source-map self-exec parity; CommonJS main-module
uncaughtException stack handling; Node global enumerable surface parity; WebStreams
BYOB invalid-state error-code parity; fs.WriteStream node:test lifecycle-drain
parity for uncorked writes; HTTP parser async-resource lifecycle parity; stream
writable same-callback TickObject harness-drain isolation; Readable compose
operator shape, Node-stream tail pumping, and async-generator destroy parity; and
source-confirmed native crypto provider-composition reclassification for the
OpenSSL-gated `SubtleCrypto.supports()` matrix; source-confirmed host-owned
TLSWRAP async-hook graph reclassification for a real TLS server/client fixture; and
source-confirmed host-owned HTTP abort/GC async-resource topology reclassification
for a real localhost server/client fixture; and source-confirmed host-owned TCP
listener/client reclassification for a stream-base socket fixture; and
source-confirmed host-owned loopback HTTP server/client reclassification for a
WebAssembly streaming fixture; and source-confirmed host process beforeExit plus
TCP-listener reclassification for a process lifecycle fixture; and source-confirmed
host subprocess/platform-probe reclassification for an fs utimes fixture; and
source-confirmed host subprocess/FIFO reclassification for an fs read-stream
fixture; and source-confirmed absolute host-root filesystem topology
reclassification for an fs realpath fixture; and runtime-local pre-epoch
filesystem timestamp parity for an fs stat date fixture; and
runtime-local symlink-entry removal authorization for fs directory/junction
symlink fixtures; and fs.promises FileHandle/assert/chown/lchmod parity for
`test-fs-promises.js`; vm dynamic-import missing-flag callback error parity
for `test-vm-dynamic-import-callback-missing-flag.js`; and vm module
dynamic-import callback option/attributes/invalid-result parity for
`test-vm-module-dynamic-import.js`; deno_core event-loop liveness parity
for unrefed setImmediate fixtures; and setImmediate queue-throw plus timer/domain
reset semantics; process/domain capture-callback ordering after `domain`
module load; and public `node:v8` promiseHooks API/ordering parity for
`createHook`, `onAfter`, `onSettled`, and hook exception routing; and VM
`microtaskMode: "afterEvaluate"` Promise queue isolation for
`test-vm-script-after-evaluate.js` while preserving normal domain/VM Promise
propagation; and the same afterEvaluate queue fix promoted
`test-vm-module-after-evaluate.js`; and Node-style weak domain retention in the
async-id pairing map promoted `test-domain-async-id-map-leak.js`; and Node-style
domain stack clearing before uncaughtException listeners, plus CommonJS-main and
beforeExit harness plumbing, promoted
`test-domain-stack-empty-in-process-uncaughtexception.js`; and Node-aligned VM
global query-trap semantics that keep outer sandbox prototype properties out of
`Object.hasOwn()` and `in` while preserving read resolution, promoted
`test-vm-global-property-prototype.js`; and Node-style VM sandbox
non-configurable property redefine failures promoted
`test-vm-global-property-interceptors.js`; and SourceTextModule evaluate timeout,
context-scoped generated module identifiers, public `util.inspect()` surface,
and abstract `Module` constructor parity promoted `test-vm-module-basic.js`;
the same module timeout parity dynamically greened and promoted
`test-vm-timeout-escape-promise-module.js`; and VM no-referrer dynamic import
fallback now routes through the context-level `importModuleDynamically` callback,
promoting `test-vm-module-referrer-realm.mjs`; all
dynamically green-guarded or structurally source-confirmed, zero false greens,
regression-verified at their promotion surface). 34 unique fixtures remain
(node22=25, node24=32).

## Genuinely blocked (cannot be reached in the V8-isolate/runtime/fork scope on this host)

- `test/parallel/test-vm-module-hastoplevelawait.js` (24) — owner: nimbus/rusty_v8 (Module::HasTopLevelAwait binding -> from-source V8 -> OOM)
- `test/parallel/test-vm-module-import-meta.js` (22+24) — owner: nimbus/deno (libs/core/runtime/bindings.rs:1104 + ext/node vm initializeImportMeta wiring)
- `test/parallel/test-webcrypto-keygen-kmac.js` (24) — owner: nimbus/deno (ext/crypto: Ed448/KMAC native primitives; may be absent from aws-lc)
- `test/parallel/test-webcrypto-sign-verify-eddsa.js` (22) — owner: nimbus/deno (ext/crypto: Ed448/KMAC native primitives; may be absent from aws-lc)
- `test/parallel/test-webcrypto-sign-verify-kmac.js` (24) — owner: nimbus/deno (ext/crypto: Ed448/KMAC native primitives; may be absent from aws-lc)

## Tractable but deep (sustained multi-session native deno_core/deno_crypto work — task #61)

### DEEP_crypto_provider (14) — owner: nimbus/deno (ext/crypto / deno_node_crypto)
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
- `test/parallel/test-webcrypto-wrap-unwrap.js` (22+24)

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

### DEEP_hang_timeout (3) — owner: nimbus/deno + nimbus-runtime
- `test/parallel/test-perf-hooks-eventlooputilization.js` (24)
- `test/parallel/test-performance-eventlooputil.js` (22)
- `test/parallel/test-webstreams-clone-unref.js` (22+24)

### DEEP_promise_hooks (2) — owner: nimbus/deno (deno_core promise hooks)
- `test/parallel/test-heapdump-async-hooks-init-promise.js` (22+24)
- `test/parallel/test-promise-swallowed-event.js` (22+24)

## Conclusion

0/0 is not reachable within a single session. The genuinely-blocked subset is a true
blocker (rusty_v8 OOM binding, deno_core import-meta panic needing cross-boundary
initializeImportMeta wiring, native Ed448/KMAC primitives maybe absent from aws-lc). The
DEEP categories are individually tractable via the proven fork-owner flow but constitute a
multi-session effort. Gate held RED and honest at node22=25 / node24=32.
