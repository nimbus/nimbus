# Loader-Context Failure Inventory

Status: `in_progress`

This file is the checked-in explicit-drift inventory for the carried
loader-context denominator while `NLC10` full validation and public closeout
work is in progress.

## Current Staged Slice Status

- Status: `carried_forward`
- Current measured subset:
  - `239` official Node22 files passed
  - `175` official Node20 files passed
  - `179` official Node24 files passed in the supported lane
  - `4` explicit Node20 supported watchpoints
  - `2` explicit Node24 supported watchpoints
  - the staged `node:domain`, `node:constants`, and `node:trace_events`
    foundation waves are now promoted in the carried denominator, with the
    `node:domain` and `node:constants` tranches green across all three carried
    lanes, the tiny `node:sys`
    alias contract is now promoted across all three lanes, the first
    Node22-default `node:sqlite` foundation subset is now promoted too, the
    first staged `node:sea` non-SEA contract is now promoted on Node22, the
    first pure Node22-default `repl.start()` foundation batch is now promoted
    too, the first Node22-default `node:wasi` validation wave is now
    promoted too, the first Node22-default `node:wasi` executable wave is
    now promoted too, the narrower Node22-default `node:wasi` argv contract
    (`test-wasi-main_args.js`) is now promoted too, the first
    Node22-default `node:wasi` filesystem wave plus the first
    Node22-default `node:wasi` preopen/file-IO wave are now promoted too,
    and the first
    Node22-default `node:cluster` worker foundation wave plus the first
    Node22-default `node:cluster` worker lifecycle/teardown wave are now
    promoted too, and the first Node22-default `node:test` helper, metadata,
    `run()`, option-validation, planning, reporter-edge, reporter-output,
    CLI-options, CLI-randomize, and CLI-rerun-failures waves are now
    promoted too

## Classified Failures

- `test/parallel/test-async-local-storage-exit-does-not-leak.js`
  - classification: `node20_supported_divergence`
  - reason: official `nodejs/node v20.20.2` still expects the old JavaScript
    `AsyncLocalStorage._propagate` hook to exist, while the current runtime
    matches the newer Node22/Node24 implementation shape that guards the hook
    behind `typeof als._propagate === "function"`
- `test/parallel/test-zlib-brotli-16GB.js`
  - classification: `node20_supported_divergence`
  - reason: the promoted Brotli/control wave is green on Node22 and Node24,
    but the official Node20 supported fixture still over-observes timer
    callbacks in the 16GB stop-early assertion path (`4 !== 1`)
- `test/parallel/test-crypto-authenticated.js` *(Node20 supported lane)*
  - classification: `node20_supported_divergence`
  - reason: the shared authenticated/wrap owner pocket is closed on Node22 and
    Node24, but the official Node20 supported fixture still expects the older
    warning ordering without `DEP0182`. The current runtime now emits the
    correct AES-GCM short-tag deprecation warning before the legacy
    `crypto.createCipher()` deprecation, so this file stays explicit as a
    Node20 supported-lane drift instead of blocking the shared denominator.
- `test/parallel/test-crypto-dh.js`
  - classification: `node20_supported_divergence`
  - reason: the promoted Node22/Node24 file now matches the newer
    unspecified-validation secret error shape, while the official Node20
    validation fixture still expects the older OpenSSL invalid-secret message
- `test/parallel/test-crypto-dh-stateless.js` *(Node24 supported lane)*
  - classification: `node24_supported_derivation_error_drift`
  - reason: the Node24 supported file adds the invalid X25519 public-key case
    and currently expects `ERR_OSSL_FAILED_DURING_DERIVATION`, while the
    current stateless DH owner path still derives successfully instead of
    surfacing the newer OpenSSL3 error
- `test/parallel/test-crypto-scrypt.js`
  - classification: `node24_supported_error_shape_drift`
  - reason: the shared-LTS `crypto` KDF/stream wave is green on Node20 and
    Node22, but the official Node24 supported file now expects
    `ERR_INCOMPATIBLE_OPTION_PAIR` for duplicate short/long option pairs while
    the current runtime still returns the older
    `ERR_CRYPTO_SCRYPT_INVALID_PARAMETER` shape
- `test/parallel/test-sqlite.js`
  - classification: `sqlite_build_profile_boundary`
  - reason: the first Node22-default `node:sqlite` foundation subset is now
    green, but the remaining upstream file still reaches `no such function:
    percentile` because the current bundled SQLCipher sqlite source does not
    contain the percentile family even after the Node-style URI/open,
    `SQLTagStore`, `gc.js`, and checked-in build-flag fixes
## Remaining Work

- The broader `node:module` corpus is not exhausted yet; this inventory now
  covers the first staged CommonJS / loader-helper wave plus the first pure
  `AsyncLocalStorage` semantics slice.
- The carried denominator now also includes the staged `node:domain`
  foundation wave under `NLC9`, and that 16-file tranche is now green across
  Node22, Node20, and Node24. The cross-lane widening did not uncover a new
  runtime seam: Node20 shares the Node22 official file contents for the whole
  batch, Node24 only diverges in `test-domain-promise.js`, and the focused
  Node20 / Node24 replays plus the broad carried lanes all stayed green after
  the staged corpus was widened. There are no remaining explicit
  `node:domain` watchpoints in the carried denominator; any broader domain
  coverage is future work by omission rather than an open staged seam.
- The carried denominator now also includes the first Node22-default
  `node:constants` foundation wave under `NLC9`. That five-file slice is now
  green after the public `node:constants` export was frozen, the internal
  constants binding was restored to Node-shaped null-prototype objects, and
  platform-unsupported fs constants like `O_NOATIME` stopped leaking into the
  public `fs.constants` surface on macOS. The first lane-aware widening pass is
  now complete too: `test-constants.js`, `test-binding-constants.js`,
  `test-process-constants-noatime.js`, `test-os-constants-signals.js`, and
  `test-uv-binding-constant.js` are now green across Node22, Node20, and
  Node24 after the lane-local fixture materialization was finished. There are
  no remaining explicit `node:constants` watchpoints in the carried
  denominator; any broader constants coverage is future work by omission rather
  than an open staged seam.
- The first Node22-default `trace_events` foundation wave is now fully
  promoted under `NLC9`. The final closeout required two more real owner
  fixes on top of the earlier `fork()` shape widening and IPC lifetime
  correction: the emulated fork child now stops presenting itself as a worker
  thread when fixtures probe `worker_threads.isMainThread`, and the
  trace-events owner path now resolves CLI categories from the current
  `process.execArgv` plus exposes a local inspector `NodeTracing` session path
  instead of falling through to `ERR_INSPECTOR_COMMAND`. The targeted
  fork-child settle postlude for `test-trace-events-api.js` keeps the fix
  narrow enough that the previously green `async_hooks` promise batch stays
  clean. The broad Node22 loader-context denominator now includes all ten
  staged files truthfully:
  `test-trace-events-api.js`, `test-trace-events-binding.js`,
  `test-trace-events-bootstrap.js`, `test-trace-events-category-used.js`,
  `test-trace-events-console.js`, `test-trace-events-dynamic-enable.js`,
  `test-trace-events-environment.js`, `test-trace-events-metadata.js`,
  `test-trace-events-none.js`, and `test-trace-events-process-exit.js`.
- The first staged `node:test` family under `NLC9` is now promoted as a
  coherent Node22-default helper, context-metadata, `run()` event-metadata,
  option-validation, planning, and syntax-error file-load wave.
  `test-runner-aliases.js`, `test-runner-typechecking.js`,
  `test-runner-custom-assertions.js`, `test-runner-get-test-context.js`,
  `test-runner-assert.js`, `test-runner-test-fullname.js`,
  `test-runner-test-filepath.js`, `test-runner-test-id.js`, and
  `test-runner-filetest-location.js`, and
  `test-runner-option-validation.js`, and `test-runner-plan.mjs`, and
  `test-runner-enqueue-file-syntax-error.js` are now green and prove the current
  export-alias contract, root-hook registration, top-level skip/todo
  settlement semantics in the checked-in compat harness, module-level
  `node:test`.`assert.register`, the public `getTestContext()` export, the
  current `t.assert` helper-key surface, the basic context metadata contract
  (`fullName`, `filePath`, suite-context injection, `passed`, `attempt`, and
  `diagnostic`), and the first in-process `test.run()` event-stream metadata
  contract (`testId`, event delivery, root file failure location, and
  root syntax-error enqueue/fail behavior), plus the first narrow
  runner-option validation contract for `timeout` and `concurrency`, plus the
  first `t.plan()` / planning contract for synchronous planning,
  subtest-counted planning, `options.plan`, and `options.wait`-driven
  `test.run()` file execution. The first staged Node22-default `node:test`
  reporter-edge, reporter-output, CLI-options, CLI-randomize, and
  CLI-rerun-failures waves are now promoted too.
  `test-runner-run-files-undefined.mjs` and
  `test-runner-import-no-scheme.js` are green and prove the truthful
  `node:test/reporters` builtin presence plus the scheme-only and bare-package
  resolution contract for `test` and `test/reporters`, while
  `test-runner-reporters.js` and `test-runner-error-reporter.js` now prove the
  first builtin reporter-selection, reporter file-output, and async reporter
  failure-propagation contract, `test-runner-cli-concurrency.js` plus
  `test-runner-cli-timeout.js` now prove the first truthful synthetic
  `node --test` default-discovery and `NODE_DEBUG=test_runner` CLI-options
  contract, and `test-runner-cli-randomize.js` now proves seeded file-order,
  root-test-order, seed-banner, and watch/rerun conflict handling in the same
  compat subprocess bridge. `test-runner-test-rerun-failures.js` is now green
  too and proves the shared `node:test` rerun-attempt/state-file path, the
  continued sibling-subtest execution after a failing child, and the local
  synthetic summary emission of suite counts. Broader `node:test` CLI,
  coverage, and unstaged reporter semantics remain future work by omission
  rather than fuzzy runner noise in the carried denominator.
- The `node:sys` family is now closed for the current staged corpus. The only
  staged upstream file is `test-sys.js`, and the cross-lane promotion stayed
  honest because the first broad replay caught a real manifest materialization
  mistake before claim time: the promoted entry initially pointed at
  nonexistent `node20` / `node22` / `node24` copies instead of the shared
  staged file. Fixing that path and rerunning the full Node22 / Node20 /
  Node24 lanes promoted the alias contract truthfully across all three lanes.
- The first Node22-default `node:sqlite` foundation wave is now partially
  promoted under `NLC9`. `test-sqlite-config.js`,
  `test-sqlite-statement-sync.js`, `test-sqlite-template-tag.js`, and
  `test-sqlite-named-parameters.js` are green in the carried denominator after
  the public URI/open semantics were restored for `file:` sqlite locations,
  `SQLTagStore.size` was aligned to the Node getter contract, `gc.js` was
  staged for the template-tag fixture, and the checked-in sqlite build-preset
  contract widened the bundled SQL function family. The only remaining staged
  file in this family is `test-sqlite.js`, which now stays explicit as a
  bundled-percentile boundary instead of a fuzzy harness failure.
- The first Node22-default `node:sea` batch is now partially promoted too.
  The staged upstream file `test-sea-get-asset-keys.js` is green and proves
  the truthful non-SEA contract: the builtin exists, `isSea()` remains false,
  and asset-key access throws Node-shaped
  `ERR_NOT_IN_SINGLE_EXECUTABLE_APPLICATION` instead of failing as a missing
  module. Broader SEA embed/asset APIs are still unstaged and should remain
  unsupported by omission until a larger family batch is classified on purpose.
- The first staged `node:wasi` validation wave is now promoted on the Node22
  default lane after the owner path replaced the constructor stub with
  Node-shaped constructor, `initialize()`, and `start()` validation
  semantics. The first executable `node:wasi` wave is promoted too, and the
  broader Node22-default argv, filesystem, and preopen/file-IO waves are now
  promoted as well: `test-wasi-main_args.js`, `test-wasi-write_file.js`,
  `test-wasi-stat.js`, `test-wasi-readdir.js`, `test-wasi-notdir.js`,
  `test-wasi-io.js`, `test-wasi-preopen_populates.js`,
  `test-wasi-fd_prestat_get_refresh.js`, and `test-wasi-cant_dotdot.js` are
  green on the carried denominator, and the local `read_file` / `freopen`
  controls are green on the same owner path. The remaining broader
  `node:wasi` surface is now unstaged future work by omission rather than an
  active explicit failure pocket in the carried denominator.
- The first pure `repl.start()` foundation batch is now promoted on the
  Node22 default lane. `test-repl-definecommand.js`, `test-repl-mode.js`,
  `test-repl-recoverable.js`, and `test-repl-reset-event.js` are green after
  the REPL owner path switched to a Node-shaped non-contextified VM context
  and the terminal preview bridge stopped dropping cross-context strict-mode
  `ReferenceError` previews.
- The first `async_hooks` execution-context wave and the first promise-hook
  core wave are now promoted, and the first four pure `zlib` slices are
  promoted too. The remaining zlib surface is now lane-specific only: the
  shared `test-zlib-invalid-input-memory.js` seam is closed, and only the
  Node20-only Brotli validation drift remains explicit. The first pure
  `crypto` foundation wave, the first shared-LTS `crypto` KDF/stream wave,
  the first shared-LTS `crypto`
  symmetric-cipher/padding wave, and the first shared-LTS `crypto` DH/ECDH
  wave are now promoted, and the lane-aware SHAKE/XOF extension is promoted
  too. The shared authenticated/wrap owner pocket is now closed; the explicit
  crypto remainder is down to lane-specific validation drifts only: one
  Node20-only authenticated warning-ordering drift, one Node24 supported-only
  `test-crypto-scrypt.js` drift, one Node24 supported-only
  `test-crypto-dh-stateless.js` derivation-error drift, and one Node20-only
  DH validation-message divergence.
  The handed-off `async_hooks` promise pocket is now fully promoted under
  `NLC8` after the bundle writer moved the official CommonJS promise fixtures
  off the embedder-only ESM evaluation path and back onto a Node-shaped sync
  require envelope. The widened pure `node:v8` helper wave is now fully
  promoted, including the lane-aware heap-space contract in
  `test-v8-stats.js`. The first `worker_threads` basics contract is promoted
  too: constructor invalid-filename shaping, simple `new Worker(...)`
  bootstrap, `MessageChannel` / `MessagePort`, `onmessage`, and
  ref/unref/hasRef behavior are green across Node22, Node20, and Node24, and
  the staged CommonJS loader pocket is now fully closed after the invalid
  native addon path learned to surface Node-shaped format errors for obviously
  non-library `.node` payloads without weakening the real FFI gate for valid
  addons. The inspector front-edge contract is now fully promoted too, so the
  current `NLC8` remainder is therefore cleaner than the first classification
  map: only the lane-only drifts remain explicit. The former shared
  `test-zlib-invalid-input-memory.js` gap is now promoted after the generic
  tick-payload retention fix in `../deno/libs/core/01_core.js`.

## Current Local Evidence

- `runtime::tests::node_compat::node22_nlc7_module_commonjs_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_async_local_storage_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_async_hooks_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_async_hooks_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_async_hooks_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_async_hooks_promise_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_async_hooks_promise_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_async_hooks_promise_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_worker_main_thread_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_worker_basic_batch_fixture`
- `runtime::tests::node_compat::node20_nlc8_worker_basic_batch_fixture`
- `runtime::tests::node_compat::node24_nlc8_worker_basic_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_worker_bootstrap_batch_fixture`
- `runtime::tests::node_compat::node20_nlc8_worker_bootstrap_batch_fixture`
- `runtime::tests::node_compat::node24_nlc8_worker_bootstrap_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_worker_contract_batch_fixture`
- `runtime::tests::node_compat::node20_nlc8_worker_contract_batch_fixture`
- `runtime::tests::node_compat::node24_nlc8_worker_contract_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_worker_message_port_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_worker_message_channel_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_module_commonjs_remainder_batch_fixture`
- `runtime::tests::node_compat::node20_nlc8_module_commonjs_remainder_batch_fixture`
- `runtime::tests::node_compat::node24_nlc8_module_commonjs_remainder_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_module_wrapper_official_watchpoint`
- `runtime::tests::node_compat::node22_nlc9_domain_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc9_domain_promise_watchpoint`
- `runtime::tests::node_compat::node22_nlc9_constants_foundation_batch_fixture`
- `runtime::tests::node_compat::node20_nlc9_constants_foundation_batch_fixture`
- `runtime::tests::node_compat::node24_nlc9_constants_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc9_sys_foundation_batch_fixture`
- `runtime::tests::node_compat::node20_nlc9_sys_foundation_batch_fixture`
- `runtime::tests::node_compat::node24_nlc9_sys_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc9_sqlite_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc9_sqlite_build_profile_watchpoint`
- `runtime::tests::node_compat::node22_nlc9_sea_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc9_wasi_validation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc9_repl_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc9_trace_events_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_v8_helper_batch_fixture`
- `runtime::tests::node_compat::node20_nlc8_v8_helper_batch_fixture`
- `runtime::tests::node_compat::node24_nlc8_v8_helper_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_v8_green_batch_fixture`
- `runtime::tests::node_compat::node20_nlc8_v8_green_batch_fixture`
- `runtime::tests::node_compat::node24_nlc8_v8_green_batch_fixture`
- `runtime::tests::node_compat::node22_nlc8_vm_basic_batch_fixture`
- `runtime::tests::node_compat::node20_nlc8_vm_basic_batch_fixture`
- `runtime::tests::node_compat::node24_nlc8_vm_basic_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_async_hooks_promise_core_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_async_hooks_promise_core_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_async_hooks_promise_core_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_zlib_foundation_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_zlib_foundation_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_zlib_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_zlib_stream_lifecycle_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_zlib_stream_lifecycle_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_zlib_stream_lifecycle_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_zlib_decompression_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_zlib_decompression_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_zlib_decompression_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_zlib_brotli_and_control_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_zlib_brotli_and_control_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_zlib_brotli_and_control_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_crypto_hash_random_foundation_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_crypto_hash_random_foundation_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_crypto_hash_random_foundation_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_crypto_kdf_and_stream_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_crypto_kdf_and_stream_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_crypto_kdf_and_stream_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_crypto_cipher_and_padding_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_crypto_cipher_and_padding_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_crypto_cipher_and_padding_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_crypto_dh_and_ecdh_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_crypto_dh_and_ecdh_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_crypto_dh_and_ecdh_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_crypto_dh_safe_prime_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_crypto_dh_safe_prime_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_crypto_dh_safe_prime_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_crypto_xof_extension_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_crypto_xof_extension_batch_fixture`
- `runtime::tests::node_compat::node22_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture`
- `runtime::tests::node_compat::node24_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture`
- `runtime::tests::node_compat::node20_nlc7_crypto_authenticated_supported_watchpoint_batch`
- `runtime::tests::node_compat::node22_nlc7_crypto_dh_and_ecdh_watchpoint_batch`
- `runtime::tests::node_compat::node20_nlc7_crypto_dh_supported_watchpoint_batch`
- `runtime::tests::node_compat::node24_nlc7_crypto_scrypt_watchpoint`
- `runtime::tests::node_compat::node24_nlc7_async_local_storage_batch_fixture`
- `runtime::tests::node_compat::node22_default_lane_executes_manifested_loader_context_subset`
- `runtime::tests::node_compat::node20_supported_lane_executes_official_loader_context_subset`
- `runtime::tests::node_compat::node24_supported_lane_executes_manifested_loader_context_subset`
- `runtime::tests::node_compat::node20_async_local_storage_exit_does_not_leak_watchpoint`
- `docs/architecture/runtime/node-lts-compat/manifests/loader-context.md`
