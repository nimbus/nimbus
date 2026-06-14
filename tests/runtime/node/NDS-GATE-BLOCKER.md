# NDS gate - FORMAL documented blocked state (cycle-95, 2026-06-14)

**Branch/PR:** worktree node-default-runtime-support-hardening -> PR #10  
**Fork:** nimbus/deno v2.8.3-nimbus.42 (3e55fee636); nimbus/rusty_v8 stock v149.4.0-nimbus.1  
**Verifier:** `bash scripts/verify-node-default-runtime-support-hardening.sh` step 9

## Unsatisfied gate

Step 9 needs both lanes `gaps==0` AND `pass_rate==100`. Current generated posture: **node22 = 3**, **node24 = 4**. Not 0/0.

Session cycles 17-95 reduced the gate 81/87 -> 3/4 by harvesting every cheap/clean/
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
promoting `test-vm-module-referrer-realm.mjs`; and WebCrypto AES
`generateKey()` avoids userland Promise prototype pollution while preserving the
native-op await path, promoting `test-webcrypto-promise-prototype-pollution.mjs`;
ESM CJS named export error parity promoted `test-esm-cjs-named-error.mjs`; and
`test-performance-many-marks.js` was source-confirmed as an
isolate-execution-watchdog fairness boundary after both required lanes
terminated under the default multi-tenant isolate deadline; and
`test-v8-serialize-leak.js` was source-confirmed as a host-process RSS/GC leak
diagnostic after both required lanes already showed watchdog termination; and
runtime-local `perf_hooks.eventLoopUtilization()` cumulative/delta parity
promoted the node22 `test-performance-eventlooputil.js` and node24
`test-perf-hooks-eventlooputilization.js` hang-timeout pair; and
nimbus/deno `v2.8.3-nimbus.25` unrefs the internal transferred-WebStreams
MessagePorts after cross-realm bridge setup, promoting
`test-webstreams-clone-unref.js` in both required lanes; and
nimbus/deno `v2.8.3-nimbus.26` preserves duplicate promise settle callbacks
through deno_core and maps them to Node's deprecated process
`multipleResolves` event with DEP0160 warning behavior, promoting
`test-promise-swallowed-event.js` in both required lanes; and
nimbus/deno `v2.8.3-nimbus.27` aligns WebCrypto EC/RSA import/export
validation and error text with Node, promoting
`test-webcrypto-export-import-ec.js` and
`test-webcrypto-export-import-rsa.js` in both required lanes; and
nimbus/deno `v2.8.3-nimbus.28` adds Ed448 import/export support and fixes X448
public-key derivation, promoting `test-webcrypto-export-import-cfrg.js` in both
required lanes; and nimbus/deno `v2.8.3-nimbus.29` aligns WebCrypto HMAC import
error codes/messages, promoting node22 `test-webcrypto-export-import.js`; and
nimbus/deno `v2.8.3-nimbus.30` aligns WebCrypto `generateKey()` RSA/AES
validation, Ed448 key generation, and node:crypto utility parity, promoting
node22 `test-webcrypto-keygen.js` while the node24 copy still reaches KMAC
native-provider support; and nimbus/deno `v2.8.3-nimbus.31` aligns WebCrypto
HKDF `deriveBits()`/`deriveKey()` length, missing-option, usage, mismatch, and
AES-OCB derived-key parity, promoting `test-webcrypto-derivebits-hkdf.js` in
both required lanes; and nimbus/deno `v2.8.3-nimbus.32` hides native parentless
implementation promises from user async_hooks while keeping real user-created
nested promises visible, promoting
`test-heapdump-async-hooks-init-promise.js` in both required lanes; and
nimbus/deno `v2.8.3-nimbus.33` aligns WebCrypto wrap/unwrap validation
messages, EC exportKey wrong-format errors, and AES-KW JWK padding behavior,
promoting `test-webcrypto-wrap-unwrap.js` in both required lanes; and
nimbus/deno `v2.8.3-nimbus.34` aligns Node24 WebCrypto AES encrypt/decrypt
validation messages and AES-GCM nonce handling, promoting node24
`test-webcrypto-encrypt-decrypt-aes.js`; and cycle86 source-confirms
`test-crypto-des3-wrap.js` as Node/OpenSSL native cipher inventory rather than
portable V8-isolate Application API behavior after both required lanes reached
the official fixture's own `common.skip("des3-wrap cipher is not available")`;
and nimbus/deno `v2.8.3-nimbus.35` adds WebCrypto KMAC128/KMAC256 support,
canonicalizes CryptoKey usage ordering/deduplication, aligns ECDH
too-short-derived-bit error text, and returns copied RSA publicExponent
metadata, promoting node24 `test-webcrypto-keygen-kmac.js`,
`test-webcrypto-sign-verify-kmac.js`, `test-webcrypto-deduplicate-usages.js`,
`test-webcrypto-derivekey.js`, `test-webcrypto-export-import.js`, and
`test-webcrypto-keygen.js`; and nimbus/deno `v2.8.3-nimbus.36` adds WebCrypto
Ed448 sign/verify support and aligns EdDSA wrong-key/wrong-algorithm error text,
promoting node22 `test-webcrypto-sign-verify-eddsa.js`; and
nimbus/deno `v2.8.3-nimbus.37` adds AES-CCM authenticated cipher support and
aligns authenticated-cipher error metadata/DataView input handling, promoting
`test-crypto-authenticated.js` in both required lanes; and nimbus/deno
`v2.8.3-nimbus.38` snapshots CommonJS exports for generated ESM wrappers,
promoting `test-esm-snapshot.mjs` in both required lanes; and cycle91 gives the
broad official WebCrypto sign/verify matrix the same finite slow-fixture
evidence budget as wrap/unwrap, promoting `test-webcrypto-sign-verify.js` in
both required lanes; and nimbus/deno `v2.8.3-nimbus.39` adds Node-style
`ERR_REQUIRE_ESM_RACE_CONDITION` parity for synchronous CJS `require()` entering
an ES module while a dynamic import graph is still pending, promoting node24
`test-esm-require-race-condition.js`; and nimbus/deno `v2.8.3-nimbus.40`
defers nextTick draining while a traced CommonJS dynamic import settles,
promoting `test-esm-dynamic-import-commonjs.js` in both required lanes; and
nimbus/deno `v2.8.3-nimbus.41` shares that nextTick deferral counter with
deno_core so ESM-origin dynamic imports of CommonJS modules resume their import
continuation before `process.nextTick`, promoting
`test-esm-dynamic-import-commonjs.mjs` in both required lanes; and nimbus/deno
`v2.8.3-nimbus.42` rejects no-referrer dynamic imports without a callback with
Node's `ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING`, promoting
`test-esm-dynamic-import.js` in both required lanes;
all
dynamically green-guarded or structurally source-confirmed, zero false greens,
regression-verified at their promotion surface). 4 unique fixtures remain
(node22=3, node24=4).

## Genuinely blocked (cannot be reached in the V8-isolate/runtime/fork scope on this host)

- `test/parallel/test-vm-module-hastoplevelawait.js` (24) — owner: nimbus/rusty_v8 (Module::HasTopLevelAwait binding -> from-source V8 -> OOM)
- `test/parallel/test-vm-module-import-meta.js` (22+24) — owner: nimbus/deno (libs/core/runtime/bindings.rs:1104 + ext/node vm initializeImportMeta wiring)

## Tractable but deep (sustained multi-session deno_core/module-loader work — task #61)

### DEEP_esm_loader (2) — owner: nimbus/deno (deno_core module loader)
- `test/es-module/test-esm-loader-mock.mjs` (22+24)
- `test/es-module/test-esm-virtual-json.mjs` (22+24)

## Conclusion

0/0 is not reachable within a single session. The genuinely-blocked subset is a true
blocker (rusty_v8 OOM binding, deno_core import-meta panic needing cross-boundary
initializeImportMeta wiring). The
DEEP categories are individually tractable via the proven fork-owner flow but constitute a
multi-session effort. Gate held RED and honest at node22=3 / node24=4.
