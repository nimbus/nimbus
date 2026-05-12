# Node Compatibility Surface Matrix

Status: snapshot (updated 2026-05-11)

This matrix is the checked-in source of truth for the currently supported
Node-facing surface in `crates/nimbus-runtime`.

It complements, but does not replace, the generated Node LTS artifact set in
[`node-lts-compat/`](node-lts-compat/node-lts-compat-summary.md):

- [`node-lts-compat-summary.md`](node-lts-compat/node-lts-compat-summary.md)
- [`node-lts-compat-matrix.csv`](node-lts-compat/node-lts-compat-matrix.csv)
- the supporting `node20` / `node22` / Deno inventory CSVs in the same folder

Use the generated baseline for broad built-in coverage truth. Use this
document for the narrower, fixture-backed Nimbus runtime contract.

The important rule is simple:

- only claim behavior that has a named fixture
- treat everything else as unsupported until a fixture proves otherwise

## Current Baseline

Nimbus currently has one runtime backend (`V8DenoCore`) and these compatibility
targets:

- `WebStandardIsolate`
- `Node20`
- `Node22`
- `Node24`

Permission posture is a separate axis. The current public permission model is:

- `RuntimeMode::Restricted`
- `RuntimeMode::Standard`
- `RuntimeMode::Privileged`

The historical `Application` and `Tooling` names are now internal
`RuntimePreset` bundles that lower to explicit `RuntimeMode + RuntimeGrants`.
They may still appear in older Node compatibility manifests and generated
evidence as workload-preset labels while that evidence vocabulary is migrated;
they should not be read as permission modes.

At this stage, the verified Node surface is still deliberately narrow. It
now includes the first capability-scoped local runtime services plus the first
scoped CommonJS/package-resolution bridge, but it is still well short of
general Node parity and should be read as an explicit contract, not an
implication of future support.

Nimbus's default compatibility baseline is `Node22`. Convex-compatible Node
actions can select `Node20`, `Node22`, or `Node24` through `convex.json`, while
Node22 remains the default until a deliberate Node24-default migration.

## Public Support-State Vocabulary

Nimbus uses these support-state labels in its public Node-facing contract:

- `Supported`
- `SupportedToolingOnly`
- `Partial`
- `StubOnly`
- `NotSupported`
- `NeedsVerification`

Current public contract:

- `Node22` is the default compatibility target.
- `Node20` and `Node24` are supported compatibility lanes with lane-local
  measured evidence and product runtime selection for Convex-compatible
  `"use node"` actions.
- Nimbus does **not** currently claim full Node built-in compatibility for any
  runtime preset or grant bundle.
- Any built-in that is `SupportedToolingOnly`, `Partial`, `StubOnly`,
  `NotSupported`, or `NeedsVerification` prevents a blanket "full Node built-in
  compatibility" claim for that target/preset pair.

## Verified Surface

| Surface | `Application + WebStandardIsolate` | `Application + Node22` | `Tooling + Node22` | Evidence |
| --- | --- | --- | --- | --- |
| `globalThis.global` | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::web_standard_target_does_not_expose_node_globals`, `runtime::tests::basic_invocation::node22_target_exposes_minimal_node_globals` |
| `globalThis.Buffer` | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::web_standard_target_does_not_expose_node_globals`, `runtime::tests::basic_invocation::node22_target_exposes_minimal_node_globals`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset`, `docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md` |
| `globalThis.Deno` | unsupported | unsupported | unsupported by the same Node22 bootstrap contract | `runtime::tests::basic_invocation::runtime_removes_deno_global_from_bundle_execution`, `runtime::tests::basic_invocation::node22_target_hides_deno_bootstrap_globals` |
| `globalThis.__bootstrap` / `globalThis.bootstrap` | unsupported | unsupported | unsupported by the same Node22 bootstrap contract | `runtime::tests::basic_invocation::runtime_removes_deno_global_from_bundle_execution`, `runtime::tests::basic_invocation::node22_target_hides_deno_bootstrap_globals` |
| `process.version` | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::web_standard_target_does_not_expose_node_globals`, `runtime::tests::basic_invocation::node22_target_exposes_minimal_node_globals` |
| `process.versions.node` | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::web_standard_target_does_not_expose_node_globals`, `runtime::tests::basic_invocation::node22_target_exposes_minimal_node_globals` |
| Core `process` metadata / warning / scheduling subset: `process.release`, `process.features`, `process.uptime()`, `process.nextTick()`, `process.emitWarning()`, and `process.on("warning")` | unsupported | supported for the currently manifested `NLC4` subset | supported by same bootstrap contract | `runtime::tests::node_compat::node22_default_lane_executes_manifested_process_and_timing_subset`, `runtime::tests::node_compat::node20_supported_lane_executes_official_process_and_timing_subset`, `runtime::tests::node_compat::node20_process_features_watchpoint`, `docs/architecture/runtime/node-lts-compat/manifests/process-and-timing.md`, `docs/architecture/runtime/node-lts-compat/failures/process-and-timing.md` |
| `process.stdout` / `process.stderr` writable stream objects | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::node22_target_exposes_minimal_node_globals`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset`, `docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md` |
| `process.on("warning")` delivery for `emitWarning()` and `require("punycode")` deprecations | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::node22_target_delivers_manual_process_warning_events`, `runtime::tests::basic_invocation::node22_target_delivers_process_warning_events_for_deprecated_modules`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset`, `docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md` |
| `process.cwd()` | unsupported | supported, scoped to the generated bundle root | supported, scoped to the app root | `runtime::tests::basic_invocation::application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes`, `runtime::tests::basic_invocation::tooling_node22_allows_allowlisted_env_and_tmp_writes` |
| `process.env` | unsupported | explicit allowlist-only capability surface: the `Application` preset grants `NODE_TLS_REJECT_UNAUTHORIZED`, while non-allowlisted names resolve as `undefined` instead of throwing | allowlist-only (`PATH`, `HOME`, `PWD`, `TMPDIR`, `TEMP`, `TMP`, `NODE_ENV`, `NODE_TLS_REJECT_UNAUTHORIZED`, `npm_config_cache`, `npm_config_user_agent`, `npm_execpath`, `ESBUILD_BINARY_PATH`, `ESBUILD_MAX_BUFFER`, `NODE_V8_COVERAGE`) | `runtime::tests::basic_invocation::application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes`, `runtime::tests::basic_invocation::application_node22_allows_tls_reject_unauthorized_env_lookup`, `runtime::tests::basic_invocation::tooling_node22_allows_allowlisted_env_and_tmp_writes` |
| `process.loadEnvFile()` | unsupported | supported for staged local env files under the generated bundle root, with Node-style missing-file and permission errors and loaded keys surfaced through the explicit runtime env overlay | supported by same bootstrap contract | `runtime::tests::basic_invocation::node22_target_load_env_file_missing_file_surfaces_node_not_found_error`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_process_and_timing_subset`, `runtime::tests::node_compat::node20_supported_lane_executes_official_process_and_timing_subset`, `docs/architecture/runtime/node-lts-compat/manifests/process-and-timing.md`, `docs/architecture/runtime/node-lts-compat/failures/process-and-timing.md` |
| `node:fs/promises.readFile` | unsupported | supported inside the generated bundle root only | supported inside scoped runtime roots (`app_root`, `generated_root`, `.nimbus/tmp`, `.nimbus/cache`) | `runtime::tests::basic_invocation::application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes`, `runtime::tests::basic_invocation::tooling_node22_allows_allowlisted_env_and_tmp_writes`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_streams_and_local_io_subset`, `runtime::tests::node_compat::node20_supported_lane_executes_official_streams_and_local_io_subset` |
| `node:fs/promises.writeFile` | unsupported | denied by the scoped Deno-family permission contract when the path escapes approved roots | supported only inside pre-existing directories under approved write roots (`generated_root`, `.nimbus/tmp`, `.nimbus/cache`) | `runtime::tests::basic_invocation::application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes`, `runtime::tests::basic_invocation::tooling_node22_allows_allowlisted_env_and_tmp_writes`, `runtime::tests::basic_invocation::tooling_node22_write_file_requires_preexisting_parent_directory`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_streams_and_local_io_subset`, `runtime::tests::node_compat::node20_supported_lane_executes_official_streams_and_local_io_subset` |
| Core `node:stream`, bundle-root-safe `node:fs`, `node:readline`, `node:tty`, and `node:os` subset: constructor/state primitives, non-socket pipeline/promises helpers, callback/promise file I/O inside approved roots, `fs.glob()`, `fs.watch()` / `fs.promises.watch()` / `watchFile()`, classic `readline.Interface`, `readline/promises.Interface`, `tty_wrap` backwards-API compatibility, and `os.EOL` | unsupported | supported for the currently manifested `NLC5` subset, with `Application` preset root restrictions and the documented later-family/networking watchpoints kept explicit | supported by the same bootstrap and approved-roots contract | `runtime::tests::node_compat::node22_default_lane_executes_manifested_streams_and_local_io_subset`, `runtime::tests::node_compat::node20_supported_lane_executes_official_streams_and_local_io_subset`, `runtime::tests::node_compat::node24_supported_lane_executes_manifested_streams_and_local_io_subset`, `docs/architecture/runtime/node-lts-compat/manifests/streams-and-local-io.md`, `docs/architecture/runtime/node-lts-compat/failures/streams-and-local-io.md` |
| Initial networking subset: `node:dns` `Resolver#getServers()` and default-result-order semantics, `node:net` IP helpers plus the first local listen/server lifecycle, no-arg `server.unref()` listen behavior, socket lifecycle, timeout, local-address, and pre-connect `end(cb)` semantics, `node:http` `http.Agent` helper behavior plus the first keepalive/socket-pool/scheduling and lifecycle/removal/abort waves, the first basic client/server request, timeout, no-arg `http.Server` option hooks, response/head, broader response/status handling (`close`, `cork`, repeated headers, multi-content-length rejection, over-the-wire status messages, invalid status-message character rejection, invalid status-code request-loop handling, `writeHead()` object/array overrides, and response-splitting protection), the first promoted crypto-gated `node:https` helper/server wave plus the follow-on `https.Agent` connection/session/global-agent wave, the next local request/server/timeout/property wave, the follow-on local `https` server lifecycle/socket wave, the next `https` client/server semantics wave, the follow-on TLS/cert-material wave (`test-https-selfsigned-no-keycertsign-no-crash.js` in all lanes plus `test-https-hwm.js` in Node22/Node24), the widened pure `node:tls` helper/local-server wave (`test-tls-basic-validations.js`, `test-tls-check-server-identity.js`, `test-tls-connect-abort-controller.js`, `test-tls-connect-allow-half-open-option.js`, `test-tls-connect-hwm-option.js` in Node22/Node24, `test-tls-connect-no-host.js`, `test-tls-connect-simple.js`, `test-tls-connect-timeout-option.js`, `test-tls-options-boolean-check.js`, and `test-tls-server-parent-constructor-options.js`), the follow-on TLS session/ticket/keylog wave (`test-https-client-resume.js`, `test-https-resume-after-renew.js`, Node22 `test-https-agent-session-reuse.js`, and Node22/Node24 `test-https-agent-keylog.js`), the follow-on shared-LTS TLS credential/local-socket/strict-auth wave (`test-https-pfx.js`, `test-https-unix-socket-self-signed.js`, and `test-https-strict.js`), the next shared-LTS pure `node:http2` header/status/options wave (`test-http2-status-code.js`, `test-http2-status-code-invalid.js`, `test-http2-multi-content-length.js`, `test-http2-response-splitting.js`, `test-http2-options-server-request.js`, `test-http2-options-server-response.js`, `test-http2-zero-length-header.js`, `test-http2-multiheaders.js`, and `test-http2-multiheaders-raw.js`), the follow-on shared-LTS `node:http2` compat request/response core wave (`test-http2-compat-serverresponse.js`, `test-http2-compat-serverresponse-end.js`, `test-http2-compat-serverresponse-write.js`, `test-http2-compat-serverresponse-writehead.js`, `test-http2-compat-serverresponse-writehead-array.js`, `test-http2-compat-serverresponse-statuscode.js`, `test-http2-compat-serverresponse-statusmessage.js`, `test-http2-compat-serverresponse-statusmessage-property.js`, `test-http2-compat-serverresponse-statusmessage-property-set.js`, `test-http2-compat-serverresponse-headers.js`, `test-http2-compat-serverrequest.js`, `test-http2-compat-serverrequest-end.js`, `test-http2-compat-serverrequest-headers.js`, `test-http2-compat-serverrequest-host.js`, `test-http2-compat-serverrequest-pause.js`, `test-http2-compat-serverrequest-pipe.js`, `test-http2-compat-serverrequest-settimeout.js`, and `test-http2-compat-serverrequest-trailers.js`), the follow-on shared-LTS `node:http2` compat server-response lifecycle wave (`test-http2-compat-serverresponse-close.js`, `test-http2-compat-serverresponse-destroy.js`, `test-http2-compat-serverresponse-drain.js`, `test-http2-compat-serverresponse-end-after-statuses-without-body.js`, `test-http2-compat-serverresponse-finished.js`, `test-http2-compat-serverresponse-flushheaders.js`, `test-http2-compat-serverresponse-headers-after-destroy.js`, `test-http2-compat-serverresponse-headers-send-date.js`, `test-http2-compat-serverresponse-settimeout.js`, `test-http2-compat-serverresponse-trailers.js`, `test-http2-compat-write-early-hints.js`, `test-http2-compat-write-early-hints-invalid-argument-type.js`, `test-http2-compat-write-early-hints-invalid-argument-value.js`, and `test-http2-compat-write-head-destroyed.js`), the follow-on shared-LTS compat request-control, push, and socket wave (`test-http2-compat-aborted.js`, `test-http2-compat-client-upload-reject.js`, `test-http2-compat-errors.js`, `test-http2-compat-expect-continue-check.js`, `test-http2-compat-expect-continue.js`, `test-http2-compat-expect-handling.js`, `test-http2-compat-method-connect.js`, `test-http2-compat-serverresponse-createpushresponse.js`, `test-http2-compat-short-stream-client-server.js`, `test-http2-compat-socket-destroy-delayed.js`, `test-http2-compat-socket-set.js`, and `test-http2-compat-socket.js`), the first pure `node:http2` utility helpers including pseudoheader validation, `getPackedSettings()`, headers-list, options-buffer, and misc util coverage, the first five pure `node:dgram` waves covering helper/argument-validation, bind/address/lifecycle/ref, connected-send/default-host, broader send/callback/default-host/`sendto` semantics, and the broader local-socket/fd/multicast/error wave, callback-style local-TCP `stream.finished()`, and shared-LTS callback-style `stream.pipeline()` request/server/socket behavior plus pinned `Application`-preset package canaries for `express`, `fastify`, `socket.io`, `undici`, and `axios` in the Node22 default lane and `express` / `fastify` in the Node20 supported lane | unsupported | supported for the currently manifested `NLC6` subset, with the explicit `dgram` cluster/process, host/preset, and `reusePort` boundaries, the cross-family `process.report` / embedded `process.exit()` dependency in `test-http-agent-reuse-drained-socket-only.js`, the Node20-only `https-hwm` and `tls-connect-hwm-option` validation divergences, the legacy TLSv1.1 `https-agent-additional-options` watchpoint, and supported-lane drift kept explicit | supported by the same bootstrap contract | `runtime::tests::node_compat::node22_default_lane_executes_manifested_networking_subset`, `runtime::tests::node_compat::node20_supported_lane_executes_official_networking_subset`, `runtime::tests::node_compat::node24_supported_lane_executes_manifested_networking_subset`, `runtime::tests::basic_invocation::application_node22_networking_package_canary_batch`, `runtime::tests::basic_invocation::application_node20_networking_supported_canary_batch`, `runtime::tests::node_compat::node20_tls_connect_hwm_option_watchpoint`, `docs/architecture/runtime/node-lts-compat/manifests/networking.md`, `docs/architecture/runtime/node-lts-compat/failures/networking.md` |
| Core semantics builtin imports: `node:assert`, `node:assert/strict`, `node:buffer`, `node:console`, `node:events`, `node:punycode`, `node:querystring`, `node:string_decoder`, `node:path/posix`, `node:path/win32` | unsupported | supported | supported by the same Node22 bootstrap contract | `runtime::tests::basic_invocation::node22_target_supports_core_semantics_builtins_and_subpaths`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset`, `docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md` |
| Core `timers`, `node:util`, basic `node:diagnostics_channel`, and current `node:perf_hooks` subset: timer ordering/overflow semantics, `util.deprecate()`, `util.format()`, `util.inherits()`, `util.parseEnv()`, `util.MIMEType`, `util.MIMEParams`, `TextDecoder`, channel pub/sub / sync-unsubscribe, `PerformanceMark`, `PerformanceMeasure`, `PerformanceResourceTiming`, `performance.mark()`, `clearMarks()`, `markResourceTiming()`, `createHistogram()`, and the current `monitorEventLoopDelay()` clone/summary contract | unsupported | supported for the currently manifested `NLC4` subset | supported by same bootstrap contract | `runtime::tests::node_compat::node22_default_lane_executes_manifested_process_and_timing_subset`, `runtime::tests::node_compat::node20_supported_lane_executes_official_process_and_timing_subset`, `runtime::tests::node_compat::node20_perf_hooks_resourcetiming_watchpoint`, `docs/architecture/runtime/node-lts-compat/manifests/process-and-timing.md`, `docs/architecture/runtime/node-lts-compat/failures/process-and-timing.md` |
| `node:path` builtin import and core path helpers | unsupported | supported | supported by the same Node22 bootstrap contract | `runtime::tests::basic_invocation::node22_target_supports_node_path_builtin_imports` |
| `node:url` builtin import and core URL helpers | unsupported | supported | supported by the same Node22 bootstrap contract | `runtime::tests::basic_invocation::node22_target_supports_core_semantics_builtins_and_subpaths`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset`, `docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md` |
| `node:module.createRequire()`, staged CommonJS / JSON `Module._load()` resolution, the first public loader helpers (`builtinModules`, `isBuiltin()`, `_nodeModulePaths()`, `_resolveLookupPaths()`, and `Module._stat()`), the expanded CommonJS remainder promotion (`test-module-loading-error.js`, `test-module-loading-globalpaths.js`, `test-module-main-fail.js`, `test-module-main-extension-lookup.js`, `test-module-prototype-mutation.js`, `test-module-wrap.js`, and `test-module-wrapper.js`), the first pure `AsyncLocalStorage` semantics subset (`bind`, `contexts`, `deep-stack`, `snapshot`, and Node22/Node24 `exit-does-not-leak`), the first pure `async_hooks` execution-context wave (`AsyncResource` constructor validation, enable/disable toggles, `runInAsyncScope()` recursion/`this` semantics, `executionAsyncResource()`, await propagation, and the staged recursive init count contract), the first promise-hook core wave (`async-await`, promise-hook switching, and promise enable/disable toggles) plus the promoted promise-lifecycle continuation cases (`test-async-hooks-enable-before-promise-resolve.js`, `test-async-hooks-enable-during-promise.js`, `test-async-hooks-disable-during-promise.js`, `test-async-hooks-promise-triggerid.js`, and `test-async-hooks-promise.js`), the first promoted `worker_threads` basics contract (`test-worker-type-check.js`, `test-worker.js`, `test-worker-message-channel.js`, `test-worker-message-port.js`, `test-worker-onmessage.js`, `test-worker-ref.js`, and `test-worker-hasref.js`) plus the promoted worker bootstrap/process wave (`test-worker-execargv.js`, `test-worker-execargv-invalid.js`, `test-worker-process-argv.js`, `test-worker-process-env.js`, `test-worker-process-env-shared.js`, `test-worker-invalid-workerdata.js`, `test-worker-relative-path.js`, and `test-worker-unsupported-path.js`), the staged `node:domain` foundation wave now green across Node22, Node20, and Node24, the first four pure `zlib` slices (foundation, stream-lifecycle/error handling, decompression/dictionary behavior, and the first Brotli/control wave: constants, convenience methods, raw-stream constructors, constructor validation, empty-buffer round-trips, string compression/decompression, invalid-input handling, no-`stream` sync safety, object-write rejection, zero-byte compression, post-error/post-write close semantics, close-inside-`data`, destroy/pipe cleanup, failed-init validation, flush/flush-drain behavior, flush-flag validation, reset-before-write support, write-after-close / write-after-end handling, dictionary-backed deflate/inflate, dictionary failures, gzip file decoding, concatenated gzip members, trailing-garbage handling, premature-end semantics, truncated-input behavior, one-byte unzip chunking, zero-`windowBits` decompression support, Brotli file/decode behavior, async kMaxLength `RangeError` shaping, crc32, long-block flush/drain behavior, interleaved flush/write ordering, Brotli invalid-argument validation, max-output-length enforcement, compression-parameter mutation, random-byte pipe integrity, sync no-`close`-event behavior, write-after-flush handling, and the weak-handle external-memory regression check), the first pure `crypto` hash/HMAC/random foundation wave (`test-crypto-hash-stream-pipe.js`, `test-crypto-from-binary.js`, `test-crypto-secret-keygen.js`, `test-crypto-encoding-validation-error.js`, `test-crypto-hmac.js`, `test-crypto-hash.js`, `test-crypto-getcipherinfo.js`, `test-crypto-oneshot-hash.js`, `test-crypto-random.js`, `test-crypto-randomfillsync-regression.js`, `test-crypto-randomuuid.js`, and `test-crypto-update-encoding.js`), the first shared-LTS `crypto` KDF/stream wave (`test-crypto-classes.js`, `test-crypto-lazy-transform-writable.js`, `test-crypto-stream.js`, `test-crypto-hkdf.js`, `test-crypto-pbkdf2.js`, and Node20/Node22 `test-crypto-scrypt.js`), the first shared-LTS `crypto` symmetric-cipher/padding wave (`test-crypto-cipheriv-decipheriv.js`, `test-crypto-padding.js`, `test-crypto-padding-aes256.js`, `test-crypto-gcm-explicit-short-tag.js`, and `test-crypto-gcm-implicit-short-tag.js`), the first shared-LTS `crypto` Diffie-Hellman / ECDH wave (`test-crypto-dh-constructor.js`, `test-crypto-dh-errors.js`, `test-crypto-dh-leak.js`, `test-crypto-dh-generate-keys.js`, `test-crypto-dh-group-setters.js`, `test-crypto-dh-modp2-views.js`, `test-crypto-dh-modp2.js`, `test-crypto-dh-odd-key.js`, `test-crypto-dh-padding.js`, `test-crypto-dh-shared.js`, Node22/Node24 `test-crypto-dh.js`, `test-crypto-ecdh-convert-key.js`, `test-crypto-dh-curves.js`, and Node20/Node22 `test-crypto-dh-stateless.js`), the lane-aware SHAKE/XOF extension (`test-crypto-default-shake-lengths.js`, Node24 `test-crypto-default-shake-lengths-oneshot.js`, and Node24 `test-crypto-oneshot-hash-xof.js`), the authenticated/wrap extension (`test-crypto-authenticated-stream.js`, Node22/Node24 `test-crypto-authenticated.js`, `test-crypto-aes-wrap.js`, and `test-crypto-des3-wrap.js`), the widened pure `node:v8` helper wave (`test-v8-version-tag.js`, `test-v8-deserialize-buffer.js`, `test-v8-serdes.js`, `test-v8-stats.js`, and `test-v8-flag-type-check.js`), the first pure `node:vm` basics wave (`test-vm-basic.js`, `test-vm-context.js`, `test-vm-run-in-new-context.js`, `test-vm-strict-mode.js`, `test-vm-not-strict.js`, and `test-vm-create-context-arg.js`), the promoted inspector front-edge contract (`test-inspector-module.js`, `test-inspector-invalid-args.js`, `test-inspector-open.js`, `test-inspector-open-port-integer-overflow.js`, and `test-inspector-enabled.js`), the widened `node:constants` tranche (`test-constants.js`, `test-binding-constants.js`, `test-process-constants-noatime.js`, `test-os-constants-signals.js`, and `test-uv-binding-constant.js`) now green across Node22, Node20, and Node24, the first fully promoted Node22-default `node:trace_events` wave (`test-trace-events-api.js`, `test-trace-events-binding.js`, `test-trace-events-bootstrap.js`, `test-trace-events-category-used.js`, `test-trace-events-console.js`, `test-trace-events-dynamic-enable.js`, `test-trace-events-environment.js`, `test-trace-events-metadata.js`, `test-trace-events-none.js`, and `test-trace-events-process-exit.js`), the promoted cross-lane `node:sys` alias contract (`test-sys.js`), the first Node22-default `node:sqlite` foundation subset (`test-sqlite-config.js`, `test-sqlite-statement-sync.js`, `test-sqlite-template-tag.js`, and `test-sqlite-named-parameters.js`), and the first Node22-default `node:sea` non-SEA contract (`test-sea-get-asset-keys.js`) | unsupported | supported for the currently manifested loader-context subset inside approved runtime roots, with the explicit Node20-only `AsyncLocalStorage._propagate` watchpoint, the Node20-only `test-zlib-brotli-16GB.js` drift, the Node20-only `test-crypto-authenticated.js` warning-ordering drift, and the Node20-only `test-crypto-dh.js` validation-message drift kept out of the measured denominator | supported for the same staged local targets inside approved runtime roots, with the Node24 supported-only `test-crypto-scrypt.js` incompatible-option error-shape drift and the Node24-only `test-crypto-dh-stateless.js` invalid-X25519 derivation-error drift | `runtime::tests::basic_invocation::application_node22_loads_commonjs_package_entries_via_esm_import`, `runtime::tests::basic_invocation::application_node22_loads_implicit_commonjs_packages_with_nested_require`, `runtime::tests::basic_invocation::application_node22_commonjs_package_can_require_core_semantics_builtins`, `runtime::tests::node_compat::node22_default_lane_executes_manifested_loader_context_subset`, `runtime::tests::node_compat::node20_supported_lane_executes_official_loader_context_subset`, `runtime::tests::node_compat::node24_supported_lane_executes_manifested_loader_context_subset`, `runtime::tests::node_compat::node20_async_local_storage_exit_does_not_leak_watchpoint`, `runtime::tests::node_compat::node22_nlc7_async_hooks_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_async_hooks_batch_fixture`, `runtime::tests::node_compat::node24_nlc7_async_hooks_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_async_hooks_promise_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_async_hooks_promise_batch_fixture`, `runtime::tests::node_compat::node24_nlc7_async_hooks_promise_batch_fixture`, `runtime::tests::node_compat::node22_nlc8_worker_bootstrap_batch_fixture`, `runtime::tests::node_compat::node20_nlc8_worker_bootstrap_batch_fixture`, `runtime::tests::node_compat::node24_nlc8_worker_bootstrap_batch_fixture`, `runtime::tests::node_compat::node22_nlc8_v8_helper_batch_fixture`, `runtime::tests::node_compat::node20_nlc8_v8_helper_batch_fixture`, `runtime::tests::node_compat::node24_nlc8_v8_helper_batch_fixture`, `runtime::tests::node_compat::node22_nlc8_v8_green_batch_fixture`, `runtime::tests::node_compat::node20_nlc8_v8_green_batch_fixture`, `runtime::tests::node_compat::node24_nlc8_v8_green_batch_fixture`, `runtime::tests::node_compat::node22_nlc8_vm_basic_batch_fixture`, `runtime::tests::node_compat::node20_nlc8_vm_basic_batch_fixture`, `runtime::tests::node_compat::node24_nlc8_vm_basic_batch_fixture`, `runtime::tests::node_compat::node22_nlc8_inspector_front_edge_batch_fixture`, `runtime::tests::node_compat::node20_nlc8_inspector_front_edge_batch_fixture`, `runtime::tests::node_compat::node24_nlc8_inspector_front_edge_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_zlib_foundation_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_zlib_stream_lifecycle_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_zlib_decompression_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_zlib_brotli_and_control_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_crypto_hash_random_foundation_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_crypto_kdf_and_stream_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_crypto_kdf_and_stream_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_crypto_cipher_and_padding_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_crypto_cipher_and_padding_batch_fixture`, `runtime::tests::node_compat::node24_nlc7_crypto_cipher_and_padding_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_crypto_dh_curves_and_stateless_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_crypto_dh_curves_and_stateless_batch_fixture`, `runtime::tests::node_compat::node24_nlc7_crypto_dh_curves_and_stateless_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_crypto_dh_safe_prime_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_crypto_dh_safe_prime_batch_fixture`, `runtime::tests::node_compat::node24_nlc7_crypto_dh_safe_prime_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_crypto_dh_supported_watchpoint_batch`, `runtime::tests::node_compat::node24_nlc7_crypto_dh_stateless_supported_watchpoint_batch`, `runtime::tests::node_compat::node24_nlc7_crypto_scrypt_watchpoint`, `runtime::tests::node_compat::node20_nlc7_crypto_xof_extension_batch_fixture`, `runtime::tests::node_compat::node24_nlc7_crypto_xof_extension_batch_fixture`, `runtime::tests::node_compat::node22_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture`, `runtime::tests::node_compat::node24_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture`, `runtime::tests::node_compat::node20_nlc7_crypto_authenticated_supported_watchpoint_batch`, `runtime::tests::node_compat::node20_nlc9_domain_foundation_batch_fixture`, `runtime::tests::node_compat::node22_nlc9_domain_foundation_batch_fixture`, `runtime::tests::node_compat::node24_nlc9_domain_foundation_batch_fixture`, `runtime::tests::node_compat::node22_nlc9_constants_foundation_batch_fixture`, `runtime::tests::node_compat::node20_nlc9_constants_foundation_batch_fixture`, `runtime::tests::node_compat::node24_nlc9_constants_foundation_batch_fixture`, `runtime::tests::node_compat::node22_nlc9_sys_foundation_batch_fixture`, `runtime::tests::node_compat::node20_nlc9_sys_foundation_batch_fixture`, `runtime::tests::node_compat::node24_nlc9_sys_foundation_batch_fixture`, `runtime::tests::node_compat::node22_nlc9_trace_events_foundation_batch_fixture`, `runtime::tests::node_compat::node22_nlc9_sqlite_foundation_batch_fixture`, `runtime::tests::node_compat::node22_nlc9_sqlite_build_profile_watchpoint`, `runtime::tests::node_compat::node22_nlc9_sea_foundation_batch_fixture`, `docs/architecture/runtime/node-lts-compat/manifests/loader-context.md`, `docs/architecture/runtime/node-lts-compat/failures/loader-context.md` |
| Local `node_modules` package resolution with `package.json` `main` / `exports` / `"type"` / import conditions | unsupported | supported inside the generated bundle root | supported inside approved resolution roots | `runtime::tests::basic_invocation::application_node22_resolves_local_esm_packages_from_scoped_node_modules`, `runtime::tests::basic_invocation::application_node22_resolves_package_exports_from_scoped_node_modules`, `runtime::tests::basic_invocation::application_node22_loads_commonjs_package_entries_via_esm_import`, `runtime::tests::basic_invocation::application_node22_loads_implicit_commonjs_packages_with_nested_require` |
| Local CommonJS package entrypoints (`.cjs` and implicit `.js`) via ESM import | unsupported | supported inside the generated bundle root | supported inside approved runtime roots | `runtime::tests::basic_invocation::application_node22_loads_commonjs_package_entries_via_esm_import`, `runtime::tests::basic_invocation::application_node22_loads_implicit_commonjs_packages_with_nested_require` |
| Nested local CommonJS `require(...)` and `require("./data.json")` inside staged packages | unsupported | supported inside the generated bundle root | supported inside approved runtime roots | `runtime::tests::basic_invocation::application_node22_loads_implicit_commonjs_packages_with_nested_require` |
| `node:child_process.spawnSync()` subprocess execution | unsupported | unsupported, including `process.execPath` self-spawn attempts | supported for exact pre-existing staged binary paths inside approved tooling roots; subprocess env inherits only the explicit JS-visible runtime env | `runtime::tests::basic_invocation::application_node22_denies_child_process_spawn_even_for_process_exec_path`, `runtime::tests::basic_invocation::tooling_node22_executes_esbuild_style_staged_binary` |
| `esbuild`-style staged dependency preset (`require("buffer").Buffer`, `node:crypto`, `node:os`, `node:tty`, staged sync subprocess) | unsupported | unsupported | supported for staged local package binaries inside approved tooling roots | `runtime::tests::basic_invocation::tooling_node22_executes_esbuild_style_staged_binary` |

## Explicitly Unsupported Right Now

These surfaces are not yet part of the runtime contract:

- ambient/global `require`
- `require(...)` of ESM targets
- most `node:` builtin usage beyond the verified `node:stream`, bundle-root-safe `node:fs`, `node:readline`, `node:tty`, `node:os`, `node:module`, `node:path`, the core semantics slice above, and tooling-preset `node:child_process` / `buffer` / `crypto` surfaces
- general `node:test` authoring and CLI semantics beyond the narrow upstream-fixture bridge used by the checked-in compatibility runner
- broader `node:worker_threads` API surface beyond the verified basics contract
- broader `node:wasi` behavior beyond the promoted validation, executable,
  argv, filesystem, and preopen/file-IO waves
- broader `node:cluster` behavior beyond the first promoted worker-foundation
  and worker lifecycle/teardown waves
- Node-API addon loading
- Node inspector child self-fork / IPC behavior and broader worker-thread APIs

Until a fixture lands, treat them as unsupported even if a transitive runtime
dependency appears to expose pieces of them upstream.

## Notes

- The runtime only reads or writes pre-existing local artifacts inside the
  approved roots above. It never fetches packages from the network or
  materializes `node_modules` at invocation time; CLI-owned staging remains the
  only place where acquisition side effects are allowed.
- The verified CommonJS bridge is intentionally scoped: it uses
  `node:module.createRequire()` plus local staged artifacts inside approved
  runtime roots, and it now includes verified `require("node:...")` access for
  the narrow core-semantics builtin set listed above. It is not a claim of
  general Node builtin parity.
- The current manifested loader-context subset now also includes the first
  basic `node:worker_threads` contract: invalid-filename constructor shaping,
  simple `new Worker(...)` bootstrap, `MessageChannel` / `MessagePort`
  semantics, `onmessage`, and ref/unref/hasRef behavior are green across
  Node22, Node20, and Node24. This is intentionally narrower than general
  worker support; broader worker-thread APIs beyond the verified basics batch
  are not part of the runtime contract yet.
- The carried loader-context denominator now also includes the first
  Node22-default `node:domain` foundation wave: add/remove behavior, timer and
  `nextTick` propagation, nested/implicit binding, intercept/bind paths, and
  the promise-rejection-to-domain-error bridge are all green on the Node22
  default lane. This is intentionally lane-scoped for now; Node20 and Node24
  have not widened into the `domain` family yet.
- The carried loader-context denominator now also includes the widened
  `node:constants` tranche: the public `node:constants` export is frozen,
  `internalBinding('constants')` now has the Node-shaped null-prototype
  surface including the empty `internal` bucket, the public macOS
  `fs.constants` export no longer leaks unsupported `O_NOATIME`, the
  `node:os` signals constant immutability path is green, and the first
  cross-lane widening pass is now fully proven too. `test-constants.js`,
  `test-binding-constants.js`, `test-process-constants-noatime.js`,
  `test-os-constants-signals.js`, and `test-uv-binding-constant.js` are now
  green across Node22, Node20, and Node24 for the staged constants tranche.
- The carried loader-context denominator now also includes the first
  Node22-default `node:sqlite` foundation subset:
  `test-sqlite-config.js`, `test-sqlite-statement-sync.js`,
  `test-sqlite-template-tag.js`, and `test-sqlite-named-parameters.js` are
  now green after the public `file:` URI/open semantics were restored for the
  sqlite path, `SQLTagStore.size` was aligned to the Node getter contract,
  and the checked-in sqlite build preset widened the bundled SQL function
  family. The remaining staged `test-sqlite.js` file stays explicit because
  the current bundled SQLCipher sqlite source still does not contain the
  percentile family that the upstream fixture validates.
- The carried loader-context denominator now also includes the first
  Node22-default `node:sea` non-SEA contract:
  `test-sea-get-asset-keys.js` is green, the builtin now exists instead of
  falling through as a missing module, `sea.isSea()` remains false in normal
  runtime execution, and `getAssetKeys()` surfaces the Node-shaped
  `ERR_NOT_IN_SINGLE_EXECUTABLE_APPLICATION` error outside a single
  executable image. This is intentionally narrow and does not yet imply
  broader SEA embed/asset support.
- The carried loader-context denominator now also includes the first
  Node22-default `node:test` helper, context-metadata, `run()`
  event-metadata, option-validation, planning, and syntax-error file-load
  wave:
  `test-runner-aliases.js`, `test-runner-typechecking.js`,
  `test-runner-custom-assertions.js`, `test-runner-get-test-context.js`,
  `test-runner-assert.js`, `test-runner-test-fullname.js`,
  `test-runner-test-filepath.js`, `test-runner-test-id.js`, and
  `test-runner-filetest-location.js`, and
  `test-runner-option-validation.js`, and `test-runner-plan.mjs`, and
  `test-runner-enqueue-file-syntax-error.js` are green and prove the current
  export-alias, root-hook registration, top-level skip/todo settlement,
  module-level `assert.register`, `getTestContext()`, `t.assert` helper
  surface, the basic context metadata contract (`fullName`, `filePath`,
  suite-context injection, `passed`, `attempt`, and `diagnostic`), and the
  first in-process `test.run()` event-stream metadata contract (`testId`,
  event delivery, root file failure location, and root syntax-error
  enqueue/fail behavior), plus the first narrow runner-option validation
  contract for `timeout` and `concurrency` and the first `t.plan()` /
  planning contract for synchronous, subtest-counted, `options.plan`, and
  `options.wait`-driven `test.run()` execution. The first staged
  Node22-default `node:test` reporter-edge, reporter-output, CLI-options,
  CLI-randomize, and CLI-rerun-failures waves are now promoted too.
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
  too and proves rerun-state-file parsing, rerun-attempt tracking,
  sibling-subtest continuation after failure, and suite-summary emission in
  that same bridge. General `node:test` authoring and
  broader CLI/coverage/unstaged reporter semantics remain outside the current
  support claim.
- The carried loader-context denominator now also includes the first
  Node22-default pure `repl.start()` foundation batch:
  `test-repl-definecommand.js`, `test-repl-mode.js`,
  `test-repl-recoverable.js`, and `test-repl-reset-event.js` are green after
  the REPL owner path switched to a Node-shaped non-contextified VM global
  and the terminal preview bridge stopped dropping strict-mode
  cross-context `ReferenceError` previews.
- The carried loader-context denominator now also includes the first
  Node22-default `node:wasi` validation wave, the first executable wave, the
  narrower Node22-default argv contract, the first filesystem wave, and the
  first preopen/file-IO wave:
  `test-wasi-options-validation.js`,
  `test-wasi-initialize-validation.js`,
  `test-wasi-start-validation.js`,
  `test-wasi-not-started.js`,
  `test-return-on-exit.js`,
  `test-wasi-stdio.js`,
  `test-wasi-main_args.js`,
  `test-wasi-write_file.js`,
  `test-wasi-stat.js`,
  `test-wasi-readdir.js`,
  `test-wasi-notdir.js`,
  `test-wasi-io.js`,
  `test-wasi-preopen_populates.js`,
  `test-wasi-fd_prestat_get_refresh.js`, and
  `test-wasi-cant_dotdot.js` are green after `node:wasi` stopped throwing a
  constructor stub, implemented Node-shaped constructor/`initialize()`/`start()`
  validation plus started-state semantics, aligned logical stdio fd mapping,
  filled the live preview1 fd/path/import surface, and restored Node-shaped
  rights propagation for preopens and file descriptors. The local `freopen`
  and `read_file` controls are green on the same owner path. Broader
  unstaged `node:wasi` behavior is still not yet part of the runtime
  contract.
- The verified core-semantics row above is also intentionally narrow: it
  proves import-and-basic-behavior support for a concrete set of builtins and
  subpaths, but it does not yet close the whole `NLC3` family or imply a
  Node-upstream pass-rate claim on its own.
- The current pinned upstream `NLC3` subset from `nimbus/deno`
  `v2.7.14-locker.19` is now green for a manifest-driven `120`-file Node22
  core-semantics batch covering `assert`, `buffer`, `console`, `events`,
  `path`, `punycode`, `querystring`, `string_decoder`, and `url`. The buffer
  family now includes the public constructor tail (`new`, `parent-property`),
  `safe-unsafe`, `sharedarraybuffer`, `swap`, and the warning/deprecation
  slices `constructor-deprecation-error`, `nopendingdep-map`, and
  `pending-deprecation`, while the console family now includes `formatTime`,
  `not-call-toString`, the no-swallow-stack-overflow path, and
  `console-tty-colors`. The latest repin also closes the imported
  `events.addAbortListener()` stop-propagation seam through the Deno web
  event system instead of the Node-internal event-target shim. The canonical
  family/count summary lives in
  `docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md`.
- The first pinned Node20 supported lane is also green for the paired
  `116`-file core-semantics subset using official `nodejs/node v20.20.2`
  fixture copies staged under `runtime/tests/node_compat_fixtures/node20/`.
  This measures Node20 compatibility for the same public-core family without
  promoting Node20 into a separate runtime target. The extra Node22-only
  `test-url-invalid-file-url-path-input.js` slice plus the extra Node22-only
  `test-assert-deep-with-error.js` and `test-assert-class-destructuring.js`
  slices are documented in the `NLC3` failure inventory because those files are
  not present in the official Node20 corpus at `v20.20.2`.
- The ignored Node24 supported lane now stages a `160`-file official
  `nodejs/node v24.15.0` subset, but it is not a green claim: the explicit
  supported-lane watchpoint run still aborts early through a `rusty_v8` weak-handle panic near
  `test-buffer-alloc.js`. That remains forward-visibility evidence only and
  does not authorize a `Node24` support claim, because the public contract
  still requires the broader matrix, canary, and closeout gates captured in
  the completed Node LTS compatibility baseline.
- The `NLC3` failure inventory now also records the active official-runtime
  watchpoints instead of burying them in failing lanes: shared `test-assert-deep.js`
  divergence, Node22-only `test-assert-partial-deep-equal.js` aborts through a
  `rusty_v8` weak-handle panic, shared detached-buffer transfer gaps in
  `test-buffer-isascii.js` and `test-buffer-isutf8.js`, the Node20-only
  `events.once(..., null)` divergence, the Node22-only `test-path-makelong.js`
  trailing-slash expectation, the Node22 `test-path-normalize.js`
  post-CVE Windows device-path expectation mismatch, and the shared
  `test-path-resolve.js` runtime gap where `win32.resolve()` currently rejects
  drive-letter-less inputs without a CWD. Those remain explicit known gaps,
  not green support claims.
- The remaining-file inventory is now explicit: no imported public-core
  official files remain unstaged for `NLC3`. The only remaining tracked items
  are the classified watchpoints in
  `docs/architecture/runtime/node-lts-compat/failures/core-semantics.md`,
  plus 16 official files that clearly belong to later process/TTY/diagnostics/
  module families and 3 upstream internal-only helpers that should not count
  toward the public built-in compatibility claim.
- The local `~/src/github.com/nodejs/node` checkout is now the canonical
  code-first source review worktree for Node20/Node22 fixture drift analysis.
  When official Node20 and official Node22 still share one fixture body,
  Nimbus stages that shared official LTS source in both lanes instead of
  keeping a fake version split or treating the Deno-vendored corpus as the
  default truth.
- A code-first drift review of official `nodejs/node v20.20.2` and
  `nodejs/node v22.15.0` `lib/url.js` showed that the legacy parser core still
  matches across both LTS lines for the invalid-port seam. Nimbus now stages
  explicit official `node22/` fixture overrides for `test-url-parse-format.js`
  and `test-url-parse-invalid-input.js`, because the Deno-vendored corpus had
  already drifted ahead to hard-throw semantics that do not match either LTS
  release. The embedded Node22 bootstrap also now installs the default warning
  printer so `process.emitWarning()` writes to `stderr` the way the upstream
  Node test corpus expects.
- The same drift review also showed that official Node20 and official Node22
  still share the exact fixture bodies for `test-url-domain-ascii-unicode.js`,
  `test-url-pathtofileurl.js`, and `test-url-fileurltopath.js`, even though the
  pinned Deno-vendored copies lag those files. Nimbus now executes one shared
  official LTS source body for those cases in both lanes, which keeps the URL
  conversion contract aligned without reintroducing per-version drift.
- The same shared-official-LTS rule now covers `test-assert-async.js`,
  `test-assert-calltracker-report.js`, `test-assert-calltracker-verify.js`,
  `test-assert-fail-deprecation.js`, `test-assert-fail.js`,
  `test-assert-first-line.js`, and `test-assert-if-error.js`. In particular,
  `test-assert-async.js` proves the current Nimbus harness can drain at least
  one upstream top-level async `node:assert` flow, `test-assert-fail-deprecation.js`
  proves `DEP0094` warning delivery through the shared `expectWarning()` path,
  and `test-assert-first-line.js` proves the bundle runner can stage checked-in
  `test/fixtures/*` helper files. The `CallTracker` pair proves the same shared
  lane can also tolerate and record the newer `DEP0173` `assert.CallTracker`
  deprecation warning without inventing a separate Node20/Node22 split.
- The same source-first batching rule now also covers
  `test-events-listener-count-with-listener.js`, which is still byte-identical
  across official `nodejs/node v20.20.2` and `nodejs/node v22.15.0` and is now
  green in both lanes. In the same batch, `test-path-resolve.js` proved to be
  a shared cross-LTS runtime gap rather than a version split, so it stays
  pinned in the `NLC3` failure inventory instead of being counted green in
  either lane.
- The first explicit split-LTS assert bodies now cover
  `test-assert-calltracker-getCalls.js`, `test-assert-checktag.js`, and
  `test-assert-typedarray-deepequal.js`. Nimbus stages both official LTS
  copies for those files directly, and the checked-in `test/common` shim
  intercepts only the harness-owned `TEST_PARALLEL` probe so the official
  Node22 `CallTracker.getCalls()` file can execute without broadening the
  public `Application` preset `process.env` contract.
- The checked-in `esbuild` package is still not a Node-API case in this repo.
  The verified `Tooling` preset path is a staged JavaScript package plus a
  staged platform binary, not a claim that Nimbus now supports general native
  addon loading. Current explicit evidence:
  `runtime::tests::basic_invocation::tooling_node22_executes_esbuild_style_staged_binary`.
- The `Tooling` preset subprocess contract is intentionally narrower than
  general Node host access: only exact pre-existing staged binaries under
  approved tooling roots are runnable, and the published `nimbus/deno`
  `v2.7.14-locker.19` family keeps subprocess env inheritance aligned with the
  explicit JS-visible runtime env instead of re-merging hidden host env.
- The completed `NLC10` Tooling canary registry now also carries pinned package
  smoke roots for `tsx`, `ts-node`, `jest`, `prisma`, and `next` on the Node22
  default lane. These are trust-layer evidence lanes, not a blanket claim that
  every package feature is fully supported in `RuntimePreset::Tooling`; consult
  the generated `target/node-compat/` canary and dashboard artifacts for the
  measured outcome shape behind each package claim.
- `RuntimePreset::Tooling` is restricted to `Node22` in code today.
- `RuntimePreset` is a workload-bundle axis, not a scheduling axis.
  `RuntimeExecutionModel` remains separate, and `RuntimeMode` plus
  `RuntimeGrants` own permission posture.
