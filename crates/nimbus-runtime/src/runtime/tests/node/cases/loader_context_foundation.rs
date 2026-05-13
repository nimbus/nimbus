const NODE22_LOADER_CONTEXT_MODULE_COMMONJS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-module-builtin.js"),
    shared_official_batch_case!("test/parallel/test-module-cache.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-create-require.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-create-require-multibyte.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-isBuiltin.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-deprecated.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-nodemodulepaths.js"),
    shared_official_batch_case!("test/parallel/test-module-relative-lookup.js"),
    shared_official_batch_case!("test/parallel/test-module-version.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-children.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-multi-extensions.js"),
    shared_official_batch_case!("test/parallel/test-module-stat.js"),
];

const NODE22_LOADER_CONTEXT_ASYNC_LOCAL_STORAGE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-local-storage-bind.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-contexts.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-deep-stack.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-snapshot.js"),
    node22_only_batch_case!(
        "test/parallel/test-async-local-storage-exit-does-not-leak.js",
        "node22/test/parallel/test-async-local-storage-exit-does-not-leak.js"
    ),
];

const NODE22_LOADER_CONTEXT_ASYNC_HOOKS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-hooks-asyncresource-constructor.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-constructor.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-disable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-disable-enable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-recursive.js"),
    shared_official_batch_case!(
        "test/parallel/test-async-hooks-recursive-stack-runInAsyncScope.js"
    ),
    shared_official_batch_case!("test/parallel/test-async-hooks-run-in-async-scope-this-arg.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-execution-async-resource.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-execution-async-resource-await.js"),
];

const NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-hooks-async-await.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-correctly-switch-promise-hook.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-disable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-before-promise-resolve.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-enable-disable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-triggerid.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise.js"),
];

const NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_CORE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-hooks-async-await.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-correctly-switch-promise-hook.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-enable-disable.js"),
];

const LOADER_CONTEXT_FOLLOWUP_WORKER_MAIN_THREAD_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-hooks-disable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-triggerid.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-file-sync.js"),
];

const LOADER_CONTEXT_FOLLOWUP_WORKER_BASIC_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-worker-type-check.js"),
    shared_official_batch_case!("test/parallel/test-worker.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-channel.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-port.js"),
    shared_official_batch_case!("test/parallel/test-worker-onmessage.js"),
    shared_official_batch_case!("test/parallel/test-worker-ref.js"),
    shared_official_batch_case!("test/parallel/test-worker-hasref.js"),
];

const LOADER_CONTEXT_FOLLOWUP_WORKER_BOOTSTRAP_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-worker-execargv.js"),
    shared_official_batch_case!("test/parallel/test-worker-execargv-invalid.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-argv.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-env.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-env-shared.js"),
    shared_official_batch_case!("test/parallel/test-worker-invalid-workerdata.js"),
    shared_official_batch_case!("test/parallel/test-worker-relative-path.js"),
    shared_official_batch_case!("test/parallel/test-worker-unsupported-path.js"),
];

const LOADER_CONTEXT_FOLLOWUP_WORKER_CONTRACT_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-worker-type-check.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-port.js"),
];

const LOADER_CONTEXT_FOLLOWUP_WORKER_MESSAGE_PORT_BATCH: &[NodeCompatBatchEntry] = &[shared_official_batch_case!(
    "test/parallel/test-worker-message-port.js"
)];

const LOADER_CONTEXT_FOLLOWUP_WORKER_MESSAGE_CHANNEL_BATCH: &[NodeCompatBatchEntry] = &[shared_official_batch_case!(
    "test/parallel/test-worker-message-channel.js"
)];

const LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH: &[NodeCompatBatchEntry] =
    &[shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-error.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    )];

const LOADER_CONTEXT_FOLLOWUP_INSPECTOR_FRONT_EDGE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-inspector-module.js"),
    shared_official_batch_case!("test/parallel/test-inspector-open.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-inspector-invalid-args.js",
        INSPECTOR_FRONT_EDGE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-inspector-open-port-integer-overflow.js"),
    shared_official_batch_case!("test/parallel/test-inspector-enabled.js"),
];

const LOADER_CONTEXT_FOLLOWUP_V8_HELPER_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-v8-version-tag.js"),
    shared_official_batch_case!("test/parallel/test-v8-deserialize-buffer.js"),
    shared_official_batch_case!("test/parallel/test-v8-serdes.js"),
    shared_official_batch_case!("test/parallel/test-v8-stats.js"),
    shared_official_batch_case!("test/parallel/test-v8-flag-type-check.js"),
];

const LOADER_CONTEXT_FOLLOWUP_V8_GREEN_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-v8-version-tag.js"),
    shared_official_batch_case!("test/parallel/test-v8-deserialize-buffer.js"),
    shared_official_batch_case!("test/parallel/test-v8-serdes.js"),
    shared_official_batch_case!("test/parallel/test-v8-flag-type-check.js"),
];

const LOADER_CONTEXT_FOLLOWUP_VM_BASIC_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-vm-basic.js"),
    shared_official_batch_case!("test/parallel/test-vm-context.js"),
    shared_official_batch_case!("test/parallel/test-vm-run-in-new-context.js"),
    shared_official_batch_case!("test/parallel/test-vm-strict-mode.js"),
    shared_official_batch_case!("test/parallel/test-vm-not-strict.js"),
    shared_official_batch_case!("test/parallel/test-vm-create-context-arg.js"),
    shared_official_batch_case!("test/parallel/test-inspector-module.js"),
    shared_official_batch_case!("test/parallel/test-inspector-open.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-inspector-invalid-args.js",
        INSPECTOR_FRONT_EDGE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-inspector-open-port-integer-overflow.js"),
    shared_official_batch_case!("test/parallel/test-inspector-enabled.js"),
];

const LOADER_CONTEXT_FOLLOWUP_VM_CONTEXT_REGRESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-script.js",
        "node22/test/parallel/test-vm-context-regression-script.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-gh1140.js",
        "node22/test/parallel/test-vm-context-regression-gh1140.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-first-line-stack.js",
        "node22/test/parallel/test-vm-context-regression-first-line-stack.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-proxy.js",
        "node22/test/parallel/test-vm-context-regression-proxy.js"
    ),
];

const LOADER_CONTEXT_FOLLOWUP_VM_CONTEXT_REMAINDER_REGRESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-null-context.js",
        "node22/test/parallel/test-vm-context-regression-null-context.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-invalid-context-args.js",
        "node22/test/parallel/test-vm-context-regression-invalid-context-args.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-regexp-throws.js",
        "node22/test/parallel/test-vm-context-regression-regexp-throws.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-delete.js",
        "node22/test/parallel/test-vm-context-regression-delete.js"
    ),
];

const NODE_TOOLS_DOMAIN_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-add-remove.js",
        "node22/test/parallel/test-domain-add-remove.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-bind-timeout.js",
        "node22/test/parallel/test-domain-bind-timeout.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee-error-listener.js",
        "node22/test/parallel/test-domain-ee-error-listener.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee-implicit.js",
        "node22/test/parallel/test-domain-ee-implicit.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee.js",
        "node22/test/parallel/test-domain-ee.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-enter-exit.js",
        "node22/test/parallel/test-domain-enter-exit.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-from-timer.js",
        "node22/test/parallel/test-domain-from-timer.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-implicit-binding.js",
        "node22/test/parallel/test-domain-implicit-binding.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-intercept.js",
        "node22/test/parallel/test-domain-intercept.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-multiple-errors.js",
        "node22/test/parallel/test-domain-multiple-errors.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-nested.js",
        "node22/test/parallel/test-domain-nested.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-nexttick.js",
        "node22/test/parallel/test-domain-nexttick.js"
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-domain-promise.js",
        node20_fixture_source_path: Some("node22/test/parallel/test-domain-promise.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-domain-promise.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-domain-promise.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-run.js",
        "node22/test/parallel/test-domain-run.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-timer.js",
        "node22/test/parallel/test-domain-timer.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-timers.js",
        "node22/test/parallel/test-domain-timers.js"
    ),
];

const NODE_TOOLS_CONSTANTS_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-constants.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-constants.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-constants.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-constants.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-binding-constants.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-binding-constants.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-binding-constants.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-binding-constants.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-process-constants-noatime.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-process-constants-noatime.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-process-constants-noatime.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-process-constants-noatime.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-os-constants-signals.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-os-constants-signals.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-os-constants-signals.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-os-constants-signals.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-uv-binding-constant.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-uv-binding-constant.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-uv-binding-constant.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-uv-binding-constant.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_TRACE_EVENTS_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-api.js",
        "node22/test/parallel/test-trace-events-api.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-api.js",
        "node22/test/parallel/test-trace-events-api.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-binding.js",
        "node22/test/parallel/test-trace-events-binding.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-bootstrap.js",
        "node22/test/parallel/test-trace-events-bootstrap.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-category-used.js",
        "node22/test/parallel/test-trace-events-category-used.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-console.js",
        "node22/test/parallel/test-trace-events-console.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-dynamic-enable.js",
        "node22/test/parallel/test-trace-events-dynamic-enable.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-environment.js",
        "node22/test/parallel/test-trace-events-environment.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-metadata.js",
        "node22/test/parallel/test-trace-events-metadata.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-none.js",
        "node22/test/parallel/test-trace-events-none.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-process-exit.js",
        "node22/test/parallel/test-trace-events-process-exit.js"
    ),
];

const NODE_TOOLS_SYS_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-sys.js",
    node20_fixture_source_path: Some("test/parallel/test-sys.js"),
    node22_fixture_source_path: Some("test/parallel/test-sys.js"),
    node24_fixture_source_path: Some("test/parallel/test-sys.js"),
    shared_extra_files: &[],
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NODE_TOOLS_SQLITE_NEXT_DB_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/sqlite/next-db.js",
        fixture_source_path: "test/sqlite/next-db.js",
    }];

const NODE_TOOLS_WASI_VALIDATION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/simple.wasm",
        fixture_source_path: "test/fixtures/simple.wasm",
    }];

const NODE_TOOLS_WASI_EXECUTION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/main_args.wasm",
        fixture_source_path: "test/wasi/wasm/main_args.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/exitcode.wasm",
        fixture_source_path: "test/wasi/wasm/exitcode.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/stdin.wasm",
        fixture_source_path: "test/wasi/wasm/stdin.wasm",
    },
];

const NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/wasi.js",
        fixture_source_path: "test/common/wasi.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/child_process.js",
        fixture_source_path: "test/common/child_process.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi-preview-1.js",
        fixture_source_path: "test/fixtures/wasi-preview-1.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/input.txt",
        fixture_source_path: "test/fixtures/wasi/input.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/input2.txt",
        fixture_source_path: "test/fixtures/wasi/input2.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/notadir",
        fixture_source_path: "test/fixtures/wasi/notadir",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/main_args.wasm",
        fixture_source_path: "test/wasi/wasm/main_args.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/write_file.wasm",
        fixture_source_path: "test/wasi/wasm/write_file.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/stat.wasm",
        fixture_source_path: "test/wasi/wasm/stat.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/readdir.wasm",
        fixture_source_path: "test/wasi/wasm/readdir.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/notdir.wasm",
        fixture_source_path: "test/wasi/wasm/notdir.wasm",
    },
];

const NODE_TOOLS_WASI_PREOPEN_IO_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/wasi.js",
        fixture_source_path: "test/common/wasi.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/child_process.js",
        fixture_source_path: "test/common/child_process.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi-preview-1.js",
        fixture_source_path: "test/fixtures/wasi-preview-1.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/input.txt",
        fixture_source_path: "test/fixtures/wasi/input.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/input2.txt",
        fixture_source_path: "test/fixtures/wasi/input2.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/freopen.wasm",
        fixture_source_path: "test/wasi/wasm/freopen.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/read_file.wasm",
        fixture_source_path: "test/wasi/wasm/read_file.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/read_file_twice.wasm",
        fixture_source_path: "test/wasi/wasm/read_file_twice.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/stdin.wasm",
        fixture_source_path: "test/wasi/wasm/stdin.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/preopen_populates.wasm",
        fixture_source_path: "test/wasi/wasm/preopen_populates.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/fd_prestat_get_refresh.wasm",
        fixture_source_path: "test/wasi/wasm/fd_prestat_get_refresh.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/cant_dotdot.wasm",
        fixture_source_path: "test/wasi/wasm/cant_dotdot.wasm",
    },
];

const NODE_TOOLS_SQLITE_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-config.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-config.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_INDEX_MJS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-statement-sync.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-statement-sync.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-template-tag.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-template-tag.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_GC_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-named-parameters.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-named-parameters.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_WASI_VALIDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-options-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-options-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-initialize-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-initialize-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_VALIDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-start-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-start-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_VALIDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_WASI_EXECUTION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-not-started.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-not-started.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-return-on-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-return-on-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-stdio.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-stdio.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-main_args.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-main_args.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-write_file.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-write_file.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-stat.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-stat.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-readdir.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-readdir.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-notdir.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-notdir.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_WASI_PREOPEN_IO_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-io.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-io.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-preopen_populates.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-preopen_populates.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-fd_prestat_get_refresh.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-fd_prestat_get_refresh.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-cant_dotdot.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-cant_dotdot.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_WASI_IO_SUBCASE_WATCHPOINT_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-freopen-only.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-freopen-only.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-read-file-only.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-read-file-only.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NODE_TOOLS_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_SEA_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-sea-get-asset-keys.js",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-sea-get-asset-keys.js"),
    node24_fixture_source_path: None,
    shared_extra_files: &[],
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NODE_TOOLS_REPL_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-definecommand.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-definecommand.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-mode.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-mode.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-recoverable.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-recoverable.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-reset-event.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-reset-event.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_TEST_RUNNER_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-aliases.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-aliases.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-typechecking.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-typechecking.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-custom-assertions.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-custom-assertions.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-get-test-context.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-get-test-context.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-assert.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-assert.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_TEST_RUNNER_CONTEXT_METADATA_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-fullname.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-fullname.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-filepath.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-filepath.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_TEST_RUNNER_RUN_EVENT_METADATA_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-id.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-id.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_EVENT_METADATA_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-filetest-location.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-filetest-location.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_EVENT_METADATA_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_TEST_RUNNER_OPTION_VALIDATION_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-runner-option-validation.js",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-runner-option-validation.js"),
    node24_fixture_source_path: None,
    shared_extra_files: &[],
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NODE_TOOLS_TEST_RUNNER_PLAN_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-runner-plan.mjs",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-runner-plan.mjs"),
    node24_fixture_source_path: None,
    shared_extra_files: COMMON_TEST_RUNNER_PLAN_EXTRA_FILES,
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NODE_TOOLS_TEST_RUNNER_RUN_EDGE_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-runner-enqueue-file-syntax-error.js",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-runner-enqueue-file-syntax-error.js"),
    node24_fixture_source_path: None,
    shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NODE_TOOLS_TEST_RUNNER_REPORTERS_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-run-files-undefined.mjs",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-run-files-undefined.mjs"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-import-no-scheme.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-import-no-scheme.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_TEST_RUNNER_REPORTER_OUTPUT_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-reporters.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-reporters.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_REPORTERS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-error-reporter.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-error-reporter.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_REPORTERS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_TEST_RUNNER_CLI_OPTIONS_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-cli-concurrency.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-cli-concurrency.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_OPTIONS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-cli-timeout.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-cli-timeout.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_OPTIONS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_TEST_RUNNER_CLI_RANDOMIZE_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-runner-cli-randomize.js",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-runner-cli-randomize.js"),
    node24_fixture_source_path: None,
    shared_extra_files: COMMON_TEST_RUNNER_CLI_RANDOMIZE_EXTRA_FILES,
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NODE_TOOLS_TEST_RUNNER_CLI_RERUN_FAILURES_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-rerun-failures.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-rerun-failures.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_RERUN_FAILURES_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

const NODE_TOOLS_CLUSTER_WORKER_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-constructor.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-constructor.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-init.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-init.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-isdead.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-isdead.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-isconnected.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-isconnected.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE_TOOLS_CLUSTER_WORKER_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-events.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-events.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-disconnect.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-disconnect.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-forced-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-forced-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-kill.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-kill.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

