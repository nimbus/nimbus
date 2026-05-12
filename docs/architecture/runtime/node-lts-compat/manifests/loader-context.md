# Loader-Context Node Test Slices

Current upstream Node test-slice manifest for the carried loader-context
denominator during active `NLC10` work.

Source corpus:

- pinned official Node22 validation corpus:
  `nodejs/node @ v22.15.0`
- pinned official Node20 supported corpus:
  `nodejs/node @ v20.20.2`
- pinned official Node24 supported corpus:
  `nodejs/node @ v24.15.0`

This file records the pinned official-fixture subset for the carried
loader-context denominator. The canonical source of truth for the executed subset is
`LOADER_CONTEXT_BATCH` in
`crates/neovex-runtime/src/runtime/tests/node/mod.rs`; this document keeps
the current green denominator resumable without rereading the Rust batch table.

## Initial Slice Map

| Family | Initial upstream test slices |
| --- | --- |
| `node:module` CommonJS / loader foundation | `test/parallel/test-module-*.js` |

## Current Manifested Official Subset

Current manifested batch counts:

- Node22 default lane: `239` official files
- Node20 supported lane: `175` official files
- Node24 supported lane: `179` staged official files

Family breakdown for the current manifested subset:

| Family | Node22 green | Node20 green | Node24 supported staged | Notes |
| --- | ---: | ---: | ---: | --- |
| `node:module` CommonJS / loader foundation plus the expanded CommonJS remainder promotion (`test-module-loading-error.js`, `test-module-loading-globalpaths.js`, `test-module-main-fail.js`, `test-module-main-extension-lookup.js`, `test-module-prototype-mutation.js`, `test-module-wrap.js`, and `test-module-wrapper.js`), the first `AsyncLocalStorage` semantics slice, the first pure `async_hooks` execution-context wave, the first promise-hook core wave, the promoted promise-lifecycle continuation cases (`test-async-hooks-enable-before-promise-resolve.js`, `test-async-hooks-enable-during-promise.js`, `test-async-hooks-disable-during-promise.js`, `test-async-hooks-promise-triggerid.js`, and `test-async-hooks-promise.js`), the first promoted `worker_threads` basics contract (`test-worker-type-check.js`, `test-worker.js`, `test-worker-message-channel.js`, `test-worker-message-port.js`, `test-worker-onmessage.js`, `test-worker-ref.js`, and `test-worker-hasref.js`), the promoted `worker_threads` bootstrap/process contract (`test-worker-execargv.js`, `test-worker-execargv-invalid.js`, `test-worker-process-argv.js`, `test-worker-process-env.js`, `test-worker-process-env-shared.js`, `test-worker-invalid-workerdata.js`, `test-worker-relative-path.js`, and `test-worker-unsupported-path.js`), the staged `node:domain` foundation wave now green across all three carried lanes (`test-domain-add-remove.js`, `test-domain-bind-timeout.js`, `test-domain-ee-error-listener.js`, `test-domain-ee-implicit.js`, `test-domain-ee.js`, `test-domain-enter-exit.js`, `test-domain-from-timer.js`, `test-domain-implicit-binding.js`, `test-domain-intercept.js`, `test-domain-multiple-errors.js`, `test-domain-nested.js`, `test-domain-nexttick.js`, `test-domain-promise.js`, `test-domain-run.js`, `test-domain-timer.js`, and `test-domain-timers.js`), the widened `node:constants` tranche (`test-constants.js`, `test-binding-constants.js`, `test-process-constants-noatime.js`, `test-os-constants-signals.js`, and `test-uv-binding-constant.js`) now green across all three carried lanes, the first fully promoted Node22-default `node:trace_events` wave (`test-trace-events-api.js`, `test-trace-events-binding.js`, `test-trace-events-bootstrap.js`, `test-trace-events-category-used.js`, `test-trace-events-console.js`, `test-trace-events-dynamic-enable.js`, `test-trace-events-environment.js`, `test-trace-events-metadata.js`, `test-trace-events-none.js`, and `test-trace-events-process-exit.js`), the newly promoted cross-lane `node:sys` alias contract (`test-sys.js`), the first Node22-default `node:sqlite` foundation subset (`test-sqlite-config.js`, `test-sqlite-statement-sync.js`, `test-sqlite-template-tag.js`, and `test-sqlite-named-parameters.js`), the first Node22-default `node:sea` non-SEA contract (`test-sea-get-asset-keys.js`), the first Node22-default pure `repl.start()` foundation batch (`test-repl-definecommand.js`, `test-repl-mode.js`, `test-repl-recoverable.js`, and `test-repl-reset-event.js`), the first Node22-default `node:wasi` validation wave (`test-wasi-options-validation.js`, `test-wasi-initialize-validation.js`, and `test-wasi-start-validation.js`), the first Node22-default `node:wasi` executable wave (`test-wasi-not-started.js`, `test-return-on-exit.js`, and `test-wasi-stdio.js`), the first Node22-default `node:wasi` argv contract (`test-wasi-main_args.js`), the first Node22-default `node:wasi` filesystem wave (`test-wasi-write_file.js`, `test-wasi-stat.js`, `test-wasi-readdir.js`, and `test-wasi-notdir.js`), the first Node22-default `node:wasi` preopen/file-IO wave (`test-wasi-io.js`, `test-wasi-preopen_populates.js`, `test-wasi-fd_prestat_get_refresh.js`, and `test-wasi-cant_dotdot.js`), the first Node22-default `node:cluster` worker foundation wave (`test-cluster-worker-constructor.js`, `test-cluster-worker-init.js`, `test-cluster-worker-isdead.js`, and `test-cluster-worker-isconnected.js`), the first Node22-default `node:cluster` worker lifecycle/teardown wave (`test-cluster-worker-events.js`, `test-cluster-worker-exit.js`, `test-cluster-worker-disconnect.js`, `test-cluster-worker-forced-exit.js`, and `test-cluster-worker-kill.js`), the first Node22-default `node:test` helper, context-metadata, `run()` event-metadata, option-validation, planning, syntax-error file-load, reporter-edge, reporter-output, CLI-options, CLI-randomize, and CLI-rerun-failures wave (`test-runner-aliases.js`, `test-runner-typechecking.js`, `test-runner-custom-assertions.js`, `test-runner-get-test-context.js`, `test-runner-assert.js`, `test-runner-test-fullname.js`, `test-runner-test-filepath.js`, `test-runner-test-id.js`, `test-runner-filetest-location.js`, `test-runner-option-validation.js`, `test-runner-plan.mjs`, `test-runner-enqueue-file-syntax-error.js`, `test-runner-run-files-undefined.mjs`, `test-runner-import-no-scheme.js`, `test-runner-reporters.js`, `test-runner-error-reporter.js`, `test-runner-cli-concurrency.js`, `test-runner-cli-timeout.js`, `test-runner-cli-randomize.js`, and `test-runner-test-rerun-failures.js`), the first four pure `zlib` slices including the promoted GC-tracking file `test-zlib-invalid-input-memory.js`, the first pure `crypto` hash/HMAC/random foundation wave, the first shared-LTS `crypto` KDF/stream wave, the first shared-LTS `crypto` symmetric-cipher/padding wave, the first shared-LTS `crypto` Diffie-Hellman / ECDH wave, the lane-aware SHAKE/XOF extension, the authenticated/wrap extension, the widened pure `node:v8` helper wave (`test-v8-version-tag.js`, `test-v8-deserialize-buffer.js`, `test-v8-serdes.js`, `test-v8-stats.js`, and `test-v8-flag-type-check.js`), and the first pure `node:vm` basics wave (`test-vm-basic.js`, `test-vm-context.js`, `test-vm-run-in-new-context.js`, `test-vm-strict-mode.js`, `test-vm-not-strict.js`, and `test-vm-create-context-arg.js`), and the promoted inspector front-edge contract (`test-inspector-module.js`, `test-inspector-invalid-args.js`, `test-inspector-open.js`, `test-inspector-open-port-integer-overflow.js`, and `test-inspector-enabled.js`) | `239` | `175` | `179` | The staged `node:domain` and `node:constants` foundation waves are now green across all three carried lanes. The `node:sys` alias contract is now green across all three lanes. The first `node:sqlite` promotion is intentionally Node22-default only, and `test-sqlite.js` stays explicit because the current bundled SQLCipher sqlite source still does not expose `percentile()` even after the URI/open, `SQLTagStore`, and checked-in build-flag fixes. The first staged `node:sea` file is now promoted too and proves the truthful non-SEA contract: `getAssetKeys()` surfaces Node-shaped `ERR_NOT_IN_SINGLE_EXECUTABLE_APPLICATION` instead of a missing builtin. The first staged `repl.start()` foundation wave is now promoted too after the REPL context switched to a Node-shaped non-contextified VM global and the terminal preview bridge stopped dropping strict-mode cross-context `ReferenceError` previews. The first staged `node:wasi` validation wave is now promoted too after `node:wasi` stopped throwing a constructor stub and now implements Node-shaped constructor, `initialize()`, and `start()` validation plus started-state semantics. The first staged `node:wasi` executable wave is now promoted too after the shared Deno owner path added Node-shaped `ERR_WASI_NOT_STARTED`, argv/import marshalling, logical-stdio-to-host-fd mapping, and the first real fd read/write/stat/seek shell for the staged wasm payloads. The broader Node22-default `node:wasi` filesystem and preopen/file-IO waves are now promoted too after the shared Deno owner path filled the live preview1 fd/path/import surface, restored Node-shaped rights propagation for preopens and file descriptors, and kept file-backed stdio writes on their real host fds instead of routing everything through `process.stdout` / `process.stderr`. The local `freopen` and `read_file` controls are green on that same owner path, so the remaining WASI surface is now broader unstaged future work rather than an active explicit failure pocket in the carried denominator. The first staged `node:cluster` worker foundation wave is now promoted too after the callable `cluster.Worker` contract, the `process.argv[1]` / `child_process.fork()` script-path seam, and the emulated-fork exit/lifecycle seams were corrected, and the first staged `node:cluster` worker lifecycle/teardown wave is now promoted too after the emulated fork child started carrying Node-shaped `listening`, `disconnect`, `exitCode`, `signalCode`, and signal-kill handshakes instead of collapsing them into generic worker-thread teardown. The first staged `node:test` helper, context-metadata, `run()` event-metadata, option-validation, planning, syntax-error file-load, reporter-edge, reporter-output, CLI-options, CLI-randomize, and CLI-rerun-failures waves are now promoted too and prove the current export-alias, root-hook registration, top-level skip/todo settlement, module-level `assert.register`, `getTestContext()`, `t.assert` helper surface, `fullName`, `filePath`, suite-context injection, basic context metadata (`passed`, `attempt`, and `diagnostic`), the first in-process `test.run()` event-stream metadata contract (`testId`, event delivery, root file failure location, and root syntax-error enqueue/fail behavior), the first narrow runner-option validation contract for `timeout` and `concurrency`, the first `t.plan()` / planning contract for synchronous, subtest-counted, `options.plan`, and `wait`-driven `test.run()` file execution, the first reporter-output contract for builtin reporter selection, file sinks, and async reporter failure propagation, the first truthful synthetic `node --test` default-discovery plus `NODE_DEBUG=test_runner` CLI-options contract, the seeded file-order, root-test-order, seed-banner, and watch/rerun conflict handling proven by `test-runner-cli-randomize.js`, and the rerun-state-file parsing, rerun-attempt tracking, sibling-subtest continuation after failure, and suite-summary emission proven by `test-runner-test-rerun-failures.js`. Broader `node:test` CLI, coverage, and unstaged reporter semantics are still outside the denominator by omission. Broader unstaged `node:wasi` families still remain outside the denominator by omission. Node20 keeps four explicit Node20 supported-lane divergences (`AsyncLocalStorage._propagate`, `test-zlib-brotli-16GB.js`, the authenticated warning-ordering drift in `test-crypto-authenticated.js`, and the older OpenSSL-message expectation in `test-crypto-dh.js`), and Node24 keeps two explicit supported-lane watchpoints (`test-crypto-scrypt.js` and `test-crypto-dh-stateless.js`). |

Current staged official files:

- `test/parallel/test-module-builtin.js`
- `test/parallel/test-module-cache.js`
- `test/parallel/test-module-create-require.js`
- `test/parallel/test-module-create-require-multibyte.js`
- `test/parallel/test-module-isBuiltin.js`
- `test/parallel/test-module-loading-deprecated.js`
- `test/parallel/test-module-nodemodulepaths.js`
- `test/parallel/test-module-relative-lookup.js`
- `test/parallel/test-module-version.js`
- `test/parallel/test-module-children.js`
- `test/parallel/test-module-multi-extensions.js`
- `test/parallel/test-module-stat.js`
- `test/parallel/test-module-loading-error.js`
- `test/parallel/test-module-loading-globalpaths.js`
- `test/parallel/test-module-main-fail.js`
- `test/parallel/test-module-main-extension-lookup.js`
- `test/parallel/test-module-prototype-mutation.js`
- `test/parallel/test-module-wrap.js`
- `test/parallel/test-module-wrapper.js`
- `test/parallel/test-async-local-storage-bind.js`
- `test/parallel/test-async-local-storage-contexts.js`
- `test/parallel/test-async-local-storage-deep-stack.js`
- `test/parallel/test-async-local-storage-snapshot.js`
- `test/parallel/test-async-local-storage-exit-does-not-leak.js` *(Node22 and Node24 only; Node20 remains an explicit validation watchpoint)*
- `test/parallel/test-async-hooks-asyncresource-constructor.js`
- `test/parallel/test-async-hooks-constructor.js`
- `test/parallel/test-async-hooks-enable-disable.js`
- `test/parallel/test-async-hooks-enable-disable-enable.js`
- `test/parallel/test-async-hooks-enable-recursive.js`
- `test/parallel/test-async-hooks-recursive-stack-runInAsyncScope.js`
- `test/parallel/test-async-hooks-run-in-async-scope-this-arg.js`
- `test/parallel/test-async-hooks-execution-async-resource.js`
- `test/parallel/test-async-hooks-execution-async-resource-await.js`
- `test/parallel/test-async-hooks-async-await.js`
- `test/parallel/test-async-hooks-correctly-switch-promise-hook.js`
- `test/parallel/test-async-hooks-disable-during-promise.js`
- `test/parallel/test-async-hooks-enable-before-promise-resolve.js`
- `test/parallel/test-async-hooks-enable-during-promise.js`
- `test/parallel/test-async-hooks-promise-enable-disable.js`
- `test/parallel/test-async-hooks-promise-triggerid.js`
- `test/parallel/test-async-hooks-promise.js`
- `test/parallel/test-worker-type-check.js`
- `test/parallel/test-worker.js`
- `test/parallel/test-worker-message-channel.js`
- `test/parallel/test-worker-message-port.js`
- `test/parallel/test-worker-onmessage.js`
- `test/parallel/test-worker-ref.js`
- `test/parallel/test-worker-hasref.js`
- `test/parallel/test-zlib-const.js`
- `test/parallel/test-zlib-convenience-methods.js`
- `test/parallel/test-zlib-create-raw.js`
- `test/parallel/test-zlib-deflate-constructors.js`
- `test/parallel/test-zlib-deflate-raw-inherits.js`
- `test/parallel/test-zlib-empty-buffer.js`
- `test/parallel/test-zlib-from-string.js`
- `test/parallel/test-zlib-invalid-input.js`
- `test/parallel/test-zlib-no-stream.js`
- `test/parallel/test-zlib-not-string-or-buffer.js`
- `test/parallel/test-zlib-object-write.js`
- `test/parallel/test-zlib-zero-byte.js`
- `test/parallel/test-zlib-close-after-error.js`
- `test/parallel/test-zlib-close-after-write.js`
- `test/parallel/test-zlib-close-in-ondata.js`
- `test/parallel/test-zlib-destroy-pipe.js`
- `test/parallel/test-zlib-destroy.js`
- `test/parallel/test-zlib-failed-init.js`
- `test/parallel/test-zlib-flush.js`
- `test/parallel/test-zlib-flush-drain.js`
- `test/parallel/test-zlib-flush-flags.js`
- `test/parallel/test-zlib-reset-before-write.js`
- `test/parallel/test-zlib-write-after-close.js`
- `test/parallel/test-zlib-write-after-end.js`
- `test/parallel/test-zlib-dictionary.js`
- `test/parallel/test-zlib-dictionary-fail.js`
- `test/parallel/test-zlib-from-gzip.js`
- `test/parallel/test-zlib-from-concatenated-gzip.js`
- `test/parallel/test-zlib-from-gzip-with-trailing-garbage.js`
- `test/parallel/test-zlib-premature-end.js`
- `test/parallel/test-zlib-truncated.js`
- `test/parallel/test-zlib-unzip-one-byte-chunks.js`
- `test/parallel/test-zlib-zero-windowBits.js`
- `test/parallel/test-zlib-brotli-16GB.js` *(Node22 and Node24 only; Node20 remains an explicit validation watchpoint)*
- `test/parallel/test-zlib-brotli-kmaxlength-rangeerror.js`
- `test/parallel/test-zlib-brotli-flush.js`
- `test/parallel/test-zlib-brotli-from-brotli.js`
- `test/parallel/test-zlib-brotli-from-string.js`
- `test/parallel/test-zlib-brotli.js`
- `test/parallel/test-zlib-crc32.js`
- `test/parallel/test-zlib-flush-drain-longblock.js`
- `test/parallel/test-zlib-flush-write-sync-interleaved.js`
- `test/parallel/test-zlib-invalid-arg-value-brotli-compress.js`
- `test/parallel/test-zlib-maxOutputLength.js`
- `test/parallel/test-zlib-params.js`
- `test/parallel/test-zlib-random-byte-pipes.js`
- `test/parallel/test-zlib-kmaxlength-rangeerror.js`
- `test/parallel/test-zlib-sync-no-event.js`
- `test/parallel/test-zlib-unused-weak.js`
- `test/parallel/test-zlib-write-after-flush.js`
- `test/parallel/test-crypto-hash-stream-pipe.js`
- `test/parallel/test-crypto-from-binary.js`
- `test/parallel/test-crypto-secret-keygen.js`
- `test/parallel/test-crypto-encoding-validation-error.js`
- `test/parallel/test-crypto-hmac.js`
- `test/parallel/test-crypto-hash.js`
- `test/parallel/test-crypto-getcipherinfo.js`
- `test/parallel/test-crypto-oneshot-hash.js`
- `test/parallel/test-crypto-random.js`
- `test/parallel/test-crypto-randomfillsync-regression.js`
- `test/parallel/test-crypto-randomuuid.js`
- `test/parallel/test-crypto-update-encoding.js`
- `test/parallel/test-crypto-classes.js`
- `test/parallel/test-crypto-lazy-transform-writable.js`
- `test/parallel/test-crypto-stream.js`
- `test/parallel/test-crypto-hkdf.js`
- `test/parallel/test-crypto-pbkdf2.js`
- `test/parallel/test-crypto-scrypt.js` *(Node20 and Node22 only; Node24 remains an explicit supported-lane watchpoint for the newer incompatible-option error shape)*
- `test/parallel/test-crypto-cipheriv-decipheriv.js`
- `test/parallel/test-crypto-padding.js`
- `test/parallel/test-crypto-padding-aes256.js`
- `test/parallel/test-crypto-gcm-explicit-short-tag.js`
- `test/parallel/test-crypto-gcm-implicit-short-tag.js`
- `test/parallel/test-crypto-dh-constructor.js`
- `test/parallel/test-crypto-dh-errors.js`
- `test/parallel/test-crypto-dh-leak.js`
- `test/parallel/test-crypto-dh-generate-keys.js`
- `test/parallel/test-crypto-dh-group-setters.js`
- `test/parallel/test-crypto-dh-modp2-views.js`
- `test/parallel/test-crypto-dh-modp2.js`
- `test/parallel/test-crypto-dh-odd-key.js`
- `test/parallel/test-crypto-dh-padding.js`
- `test/parallel/test-crypto-dh-shared.js`
- `test/parallel/test-crypto-dh.js` *(Node22 and Node24 only; Node20 remains an explicit validation watchpoint for the older invalid-secret message shape)*
- `test/parallel/test-crypto-ecdh-convert-key.js`
- `test/parallel/test-crypto-dh-curves.js`
- `test/parallel/test-crypto-dh-stateless.js` *(Node20 and Node22 only; Node24 remains an explicit supported-lane watchpoint for the invalid X25519 derivation-error shape)*
- `test/parallel/test-crypto-default-shake-lengths.js` *(Node20 and Node24 only; Node22 upstream `v22.15.0` does not ship an official file)*
- `test/parallel/test-crypto-default-shake-lengths-oneshot.js` *(Node24 only; Node20 and Node22 upstream do not ship official files)*
- `test/parallel/test-crypto-oneshot-hash-xof.js` *(Node24 only; Node20 and Node22 upstream do not ship official files)*
- `test/parallel/test-v8-version-tag.js`
- `test/parallel/test-v8-deserialize-buffer.js`
- `test/parallel/test-v8-serdes.js`
- `test/parallel/test-v8-stats.js`
- `test/parallel/test-v8-flag-type-check.js`
- `test/parallel/test-vm-basic.js`
- `test/parallel/test-vm-context.js`
- `test/parallel/test-vm-run-in-new-context.js`
- `test/parallel/test-vm-strict-mode.js`
- `test/parallel/test-vm-not-strict.js`
- `test/parallel/test-vm-create-context-arg.js`
- `test/parallel/test-inspector-module.js`
- `test/parallel/test-inspector-invalid-args.js`
- `test/parallel/test-inspector-open.js`
- `test/parallel/test-inspector-open-port-integer-overflow.js`
- `test/parallel/test-inspector-enabled.js`
- `test/parallel/test-domain-add-remove.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-bind-timeout.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-ee-error-listener.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-ee-implicit.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-ee.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-enter-exit.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-from-timer.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-implicit-binding.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-intercept.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-multiple-errors.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-nested.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-nexttick.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-promise.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-run.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-timer.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-domain-timers.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-constants.js` *(Node22 default only; Node20 and Node24 are not yet staged for this narrower file)*
- `test/parallel/test-binding-constants.js` *(Node22 default only; Node20 and Node24 are not yet staged for this narrower file)*
- `test/parallel/test-process-constants-noatime.js`
- `test/parallel/test-os-constants-signals.js` *(Node22 and Node24 only; Node20 is not yet staged for this narrower file)*
- `test/parallel/test-uv-binding-constant.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-api.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-binding.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-bootstrap.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-sys.js`
- `test/parallel/test-sqlite-config.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-sqlite-statement-sync.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-sqlite-template-tag.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-sqlite-named-parameters.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-sea-get-asset-keys.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-repl-definecommand.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-repl-mode.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-repl-recoverable.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-repl-reset-event.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-options-validation.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-initialize-validation.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-start-validation.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-not-started.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-return-on-exit.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-stdio.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-main_args.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-write_file.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-stat.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-readdir.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-notdir.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-io.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-preopen_populates.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-fd_prestat_get_refresh.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/wasi/test-wasi-cant_dotdot.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-constructor.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-init.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-isdead.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-isconnected.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-events.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-exit.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-disconnect.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-forced-exit.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-cluster-worker-kill.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-category-used.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-console.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-dynamic-enable.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-environment.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-metadata.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-none.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*
- `test/parallel/test-trace-events-process-exit.js` *(Node22 default only; Node20 and Node24 are not yet staged in the manifested denominator for this family)*

## Notes

- The first staged `node:module` wave is green because the Neovex-local module
  shim now keeps internal override specifiers out of the public
  `builtinModules` surface and restores the public `Module._stat()` contract.
- The expanded CommonJS remainder promotion now also includes
  `test-module-wrapper.js`. The canonical Deno owner fix chain in
  `ext/node/polyfills/01_require.js` aligned `Module.wrapper` replacement
  semantics in three steps: the public setter now preserves reference
  identity, the public wrapper strings now match the Node-shaped contract, and
  `Module.wrap()` now inserts an explicit newline between the wrapper prologue
  and the module source so the official no-semicolon replacement case reaches
  the intended top-level side effect during `require('./not-main-module.js')`
  after `Module.runMain()`.
- The same CommonJS family now also includes
  `test-module-loading-globalpaths.js`. That promotion required both fixture
  truth and owner fixes: the staged compat corpus now carries the official
  `test-module-loading-globalpaths/*` subtree, the subprocess harness rewrites
  copied `execPath` and env paths into each child bundle root, and the
  CommonJS loader now treats stat-only module-resolution probes beneath
  `.node_modules`, `.node_libraries`, and `lib/node` as quiet existence checks
  while still keeping actual file reads gated by the Neovex path policy.
- The same CommonJS family now also includes
  `test-module-loading-error.js`. That closeout preserved the real FFI
  capability boundary for valid native addons while fixing Node-shaped error
  precedence for obviously invalid `.node` payloads: the `.node` extension now
  does a cheap read-only native-header preflight and only converts the old
  `Requires ffi access` capability error when the staged file is already
  provably not a loadable shared library for the current platform.
- The first staged `AsyncLocalStorage` wave is also mostly green. The only
  current divergence is the official Node20
  `test-async-local-storage-exit-does-not-leak.js` expectation around the old
  JavaScript `_propagate` hook; Node22 and Node24 already match the newer
  implementation shape, so that file stays out of the Node20 denominator as a
  Node20 supported-lane watchpoint.
- The first staged `async_hooks` wave is now green on the published
  `agentstation/deno v2.7.14-locker.39` baseline after the canonical Deno
  owner fixes restored Node-style `AsyncResource` validation, execution async
  resource tracking, trigger async id semantics, and promise-hook propagation.
  The next promise-hook expansion is also green: `test-async-hooks-async-await.js`,
  `test-async-hooks-correctly-switch-promise-hook.js`,
  `test-async-hooks-enable-before-promise-resolve.js`,
  `test-async-hooks-enable-during-promise.js`,
  `test-async-hooks-disable-during-promise.js`,
  `test-async-hooks-promise-triggerid.js`, and
  `test-async-hooks-promise-enable-disable.js` all promote on the current
  canonical local Deno baseline. The harness side still stays narrow:
  Neovex-local fixture isolation disables the hook immediately after the
  user-visible `nextTick` assertion in `test-async-hooks-enable-during-promise.js`,
  while the promise-hook batch now quiesces startup Promise noise before
  synchronously requiring the CommonJS fixture for
  `test-async-hooks-disable-during-promise.js`,
  while the Deno-owner timer wrapper now emits the current callback's `after`
  when hooks become enabled mid-timeout callback in
  `test-async-hooks-enable-before-promise-resolve.js`. The staged recursive
  init case is now green too: the lane-local fixture copies disable the outer
  and nested hooks as soon as the official Node-visible `2` / `1` init counts
  are observed, which isolates the intended recursive contract from later
  embedded-runner `Immediate` tail activity without changing the public
  runtime semantics.
- The handed-off async_hooks promise pocket from the NLC7 closeout is now
  fully promoted under `NLC8`. The decisive harness-owner fix was to stop
  evaluating the official CommonJS promise fixtures at ESM top level and
  instead require them synchronously inside the sync invoke envelope after
  module evaluation has already completed. That removes the embedder-only
  module-evaluation promise from the observable hook contract without
  changing the upstream fixture assertions or the runtime's public semantics.
- The first `worker_threads` basics contract is now promoted too. The current
  manifested subset now includes `test-worker-type-check.js`,
  `test-worker.js`, `test-worker-message-channel.js`,
  `test-worker-message-port.js`, `test-worker-onmessage.js`,
  `test-worker-ref.js`, and `test-worker-hasref.js` across Node22, Node20,
  and Node24. The Deno owner fixes restored Node-shaped invalid-filename
  constructor errors, standalone `MessagePort` `ref()` / `unref()` /
  `hasRef()` behavior, custom event interop, and listener-late message
  delivery, while the Neovex runtime now always provisions a
  `SharedArrayBufferStore` and the Neovex worker bootstrap now tracks
  one-shot message listeners correctly so worker close/exit can drain
  honestly after ref/unref transitions. The explicit remainder is therefore
  sharper now: broader `worker_threads` APIs beyond this verified basics
  batch remain held out unless they are staged and proven later.
- The widened pure `node:v8` helper wave is now fully promoted. The canonical
  local Deno proof path now treats `internalBinding('js_stream')` as a real
  host-object seam instead of a fake plain-object serialization path, the
  exported `v8.Serializer` / `v8.Deserializer` constructors now surface the
  Node-shaped `ERR_CONSTRUCT_CALL_REQUIRED` contract, and the compat harness
  now injects the real Node20/Node22/Node24 lane into the bundle so `v8.ts`
  can shape heap-space filtering against the executed corpus instead of the
  fixed Node22 runtime baseline. That makes `test-v8-version-tag.js`,
  `test-v8-deserialize-buffer.js`, `test-v8-serdes.js`,
  `test-v8-stats.js`, and `test-v8-flag-type-check.js` green across Node22,
  Node20, and Node24.
- The first pure `vm` basics wave is now fully promoted as well. The earlier
  filename/stack fidelity seams in `test-vm-basic.js`, `test-vm-context.js`,
  and `test-vm-run-in-new-context.js` are closed, and the cross-lane
  `rusty_v8` weak-handle teardown abort is fixed by resetting live weak
  handles during isolate teardown instead of freeing `WeakData` before V8's
  first-pass contract is satisfied.
- The inspector front-edge contract is now fully promoted. The remaining
  harness wrinkle is narrower than a spec seam: `test-inspector-open.js` now
  runs through a worker-backed compat `fork(__filename)` child path and stays
  green in all three broad lanes, but the success-path process-exit sentinel
  still prints after the green summary. That noise is tracked as harness
  polish rather than a failing loader-context contract.
- The first pure `zlib` foundation wave is also green across Node22, Node20,
  and Node24 on the published baseline. That promoted slice now covers
  constants immutability, convenience compress/decompress helpers, raw-stream
  constructors, constructor validation, empty-buffer round-trips, string
  compression/decompression, invalid input handling, no-`stream` sync safety,
  object-write type rejection, and zero-byte compression behavior.
- The second pure `zlib` stream-lifecycle wave is green across Node22, Node20,
  and Node24 too. That promoted slice now covers post-error/post-write close
  semantics, close-inside-`data`, destroy/pipe cleanup, failed-init argument
  validation, flush and flush-drain behavior, flush-flag validation,
  reset-before-write support, and write-after-close / write-after-end error
  handling. The only extra harness materialization it required was staging the
  shared `test/fixtures/person.jpg` binary for the older Node20 `flush`
  fixture body.
- The third pure `zlib` decompression/dictionary wave is green across Node22,
  Node20, and Node24 as well. That promoted slice now covers dictionary-backed
  deflate/inflate behavior, dictionary failure modes, gzip file decoding,
  concatenated gzip members, trailing-garbage handling, premature-end
  semantics, truncated-input recovery behavior, one-byte unzip chunking, and
  zero-`windowBits` decompression support. It reuses the shared gzip fixtures
  staged under `test/fixtures/`.
- The fourth pure `zlib` Brotli/control wave is now mostly green too. That
  promoted slice now covers the first Brotli file/decode/control behavior plus
  crc32, long-block flush/drain behavior, interleaved flush/write ordering,
  Brotli invalid-argument validation, max-output-length enforcement,
  compression-parameter mutation, random-byte pipe integrity, sync
  no-`close`-event behavior, write-after-flush handling, and the promoted
  GC-tracking file `test-zlib-invalid-input-memory.js`. The decisive fix for
  that last file was not in `node:zlib` itself. The safe post-GC probe showed
  the same retention on a plain errored `Transform`, which led to the real
  owner seam in the canonical Deno sibling proof path: processed `nextTick`
  tick objects were still retaining their callback payloads after execution.
  Clearing `tock.callback`, `tock.args`, and `tock.snapshot` in
  `../deno/libs/core/01_core.js` after each tick closes the generic retention
  path without weakening the upstream fixture, and the official zlib file is
  now green in the manifested denominator. The only lane carve-out inside the
  promoted wave is the official Node20 `test-zlib-brotli-16GB.js` validation
  drift.
- The first pure `crypto` foundation wave is now green across Node22, Node20,
  and Node24 too. That promoted slice now covers stream-piped hashing,
  buffer-versus-base64 hashing parity, secret key generation, cipher encoding
  validation, HMAC constructor/basic behavior, the `Hash#digest()` error-shape
  contract, CCM `getCipherInfo()` IV-length support, `crypto.hash()`
  validation, pending-deprecation `pseudoRandomBytes` warning delivery, random
  fill regression coverage, `randomUUID()`, and `update()` encoding handling.
- The next lane-aware SHAKE/XOF extension is now promoted too. Node20 now
  carries the official `test-crypto-default-shake-lengths.js` file, and
  Node24 now also carries `test-crypto-default-shake-lengths-oneshot.js` plus
  `test-crypto-oneshot-hash-xof.js`. The shared owner fix lives in the
  canonical Deno worktree: SHAKE128/256 hashing without an explicit
  `outputLength` now emits the expected `DEP0198` pending-deprecation warning,
  which keeps the Node20 and Node24 official files green without changing the
  Node22 denominator because upstream `v22.15.0` does not ship those files.
- The first shared-LTS `crypto` KDF/stream wave is now green on Node22 and
  Node20 on the published `agentstation/deno v2.7.14-locker.39` baseline.
  That promoted slice now covers the deprecated public
  `crypto.Cipher` / `crypto.Decipher` constructor surface, legacy
  `createCipher()` / `createDecipher()` password-based key derivation,
  `aes192` / `aes-192-cbc` compatibility, lazy transform-writable behavior,
  stream error/transform contract coverage, HKDF and PBKDF2 digest support for
  the staged Node20/Node22 files, and the shared-LTS `crypto.scrypt()`
  algorithm path including zero-length keys and the default parameter path.
  The only remaining drift in this wave is supported-lane: the official Node24
  `test-crypto-scrypt.js` file expects the newer
  `ERR_INCOMPATIBLE_OPTION_PAIR` shape for duplicate short/long option pairs,
  while the current runtime still returns the older
  `ERR_CRYPTO_SCRYPT_INVALID_PARAMETER` shape accepted by the verified Node20
  and Node22 fixtures.
- The next shared-LTS `crypto` symmetric-cipher/padding wave is now green
  across Node22, Node20, and Node24 on the published
  `agentstation/deno v2.7.14-locker.39` baseline. That
  promoted slice now covers AES key-wrap cipher selection and unwrap parity
  (`test-crypto-cipheriv-decipheriv.js`), OpenSSL3-style decrypt/final error
  shaping for CBC padding failures (`test-crypto-padding.js` and
  `test-crypto-padding-aes256.js`), and the short-auth-tag GCM contract
  (`test-crypto-gcm-explicit-short-tag.js` and
  `test-crypto-gcm-implicit-short-tag.js`). The owner fixes live in the
  canonical Deno worktree and keep the public JS contract narrow: the runtime
  now recognizes the AES wrap family including `id-aes128-wrap`, rejects
  unknown ciphers through `ERR_CRYPTO_UNKNOWN_CIPHER`, rejects ECB IV misuse,
  and maps the decrypt/final failure path back to Node-style OpenSSL3 error
  codes and messages.
- The next shared-LTS `crypto` Diffie-Hellman / ECDH wave is now green across
  Node22, Node20, and Node24 for the numeric constructor surface too. That
  promoted slice now covers `test-crypto-dh-constructor.js` on all three
  lanes and the Node22/Node24 `test-crypto-dh.js` imported-prime / `verifyError`
  contract, on top of the already-green DH error validation, key generation,
  group setter behavior, MODP2 views, MODP2 shared-secret behavior, odd-key
  handling, padding behavior, shared secret computation, `ECDH.convertKey()`
  parity, and the explicit `dh-curves` regression cases. The remaining DH/ECDH
  pocket is now only lane-specific: the older Node20 `test-crypto-dh.js`
  fixture remains an explicit Node20 supported-lane divergence for the prior
  OpenSSL invalid-secret message, and only the Node24 supported lane still
  holds `test-crypto-dh-stateless.js` out for the invalid X25519
  derivation-error shape. The promoted authenticated/wrap extension now also
  covers `test-crypto-authenticated-stream.js`, `test-crypto-aes-wrap.js`,
  `test-crypto-des3-wrap.js`, and the shared Node22/Node24
  `test-crypto-authenticated.js` file. The old shared authenticated/wrap owner
  pocket is gone. The only remaining authenticated divergence is lane-specific:
  the official Node20 `test-crypto-authenticated.js` file still expects the
  older warning ordering without `DEP0182`, so it stays out of the Node20
  validation denominator as an explicit watchpoint instead of blocking the
  shared crypto surface.
- This document now carries the closed `NLC7`-through-`NLC9` loader-context
  denominator forward while `NLC10` full validation and public closeout work
  is in progress. Broader unstaged families still need the same batch-first
  classification before they can enter the denominator, but the `NLC9`
  long-tail family gate is now closed.
