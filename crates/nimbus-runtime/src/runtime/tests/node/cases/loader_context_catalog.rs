const LOADER_CONTEXT_BATCH: &[NodeCompatBatchEntry] = &[
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
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-error.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-globalpaths.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-main-fail.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-main-extension-lookup.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-prototype-mutation.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-wrap.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-wrapper.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-async-local-storage-bind.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-contexts.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-deep-stack.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-snapshot.js"),
    node22_only_batch_case!(
        "test/parallel/test-async-local-storage-exit-does-not-leak.js",
        "node22/test/parallel/test-async-local-storage-exit-does-not-leak.js"
    ),
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
    shared_official_batch_case!("test/parallel/test-async-hooks-async-await.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-correctly-switch-promise-hook.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-disable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-before-promise-resolve.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-enable-disable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-triggerid.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise.js"),
    shared_official_batch_case!("test/parallel/test-worker-type-check.js"),
    shared_official_batch_case!("test/parallel/test-worker.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-channel.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-port.js"),
    shared_official_batch_case!("test/parallel/test-worker-onmessage.js"),
    shared_official_batch_case!("test/parallel/test-worker-ref.js"),
    shared_official_batch_case!("test/parallel/test-worker-hasref.js"),
    shared_official_batch_case!("test/parallel/test-worker-execargv.js"),
    shared_official_batch_case!("test/parallel/test-worker-execargv-invalid.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-argv.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-env.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-env-shared.js"),
    shared_official_batch_case!("test/parallel/test-worker-invalid-workerdata.js"),
    shared_official_batch_case!("test/parallel/test-worker-relative-path.js"),
    shared_official_batch_case!("test/parallel/test-worker-unsupported-path.js"),
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
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sys.js",
        node20_fixture_source_path: Some("test/parallel/test-sys.js"),
        node22_fixture_source_path: Some("test/parallel/test-sys.js"),
        node24_fixture_source_path: Some("test/parallel/test-sys.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
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
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sea-get-asset-keys.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sea-get-asset-keys.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
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
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-option-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-option-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-plan.mjs",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-plan.mjs"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_PLAN_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-enqueue-file-syntax-error.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-enqueue-file-syntax-error.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
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
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-cli-randomize.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-cli-randomize.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_RANDOMIZE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-rerun-failures.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-rerun-failures.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_RERUN_FAILURES_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
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
    shared_official_batch_case!("test/parallel/test-zlib-const.js"),
    shared_official_batch_case!("test/parallel/test-zlib-convenience-methods.js"),
    shared_official_batch_case!("test/parallel/test-zlib-create-raw.js"),
    shared_official_batch_case!("test/parallel/test-zlib-deflate-constructors.js"),
    shared_official_batch_case!("test/parallel/test-zlib-deflate-raw-inherits.js"),
    shared_official_batch_case!("test/parallel/test-zlib-empty-buffer.js"),
    shared_official_batch_case!("test/parallel/test-zlib-from-string.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-input.js"),
    shared_official_batch_case!("test/parallel/test-zlib-no-stream.js"),
    shared_official_batch_case!("test/parallel/test-zlib-not-string-or-buffer.js"),
    shared_official_batch_case!("test/parallel/test-zlib-object-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-byte.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-error.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-in-ondata.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy-pipe.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy.js"),
    shared_official_batch_case!("test/parallel/test-zlib-failed-init.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-flush.js",
        COMMON_PERSON_JPG_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-flags.js"),
    shared_official_batch_case!("test/parallel/test-zlib-reset-before-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-close.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary-fail.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-concatenated-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-from-gzip-with-trailing-garbage.js"),
    shared_official_batch_case!("test/parallel/test-zlib-premature-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-truncated.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unzip-one-byte-chunks.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-windowBits.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-zlib-brotli-16GB.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-zlib-brotli-16GB.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-zlib-brotli-16GB.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-flush.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-from-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-brotli-from-string.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-crc32.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain-longblock.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-write-sync-interleaved.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-arg-value-brotli-compress.js"),
    shared_official_batch_case!("test/parallel/test-zlib-maxOutputLength.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-params.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-random-byte-pipes.js"),
    shared_official_batch_case!("test/parallel/test-zlib-sync-no-event.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unused-weak.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-flush.js"),
    shared_official_batch_case!("test/parallel/test-crypto-hash-stream-pipe.js"),
    shared_official_batch_case!("test/parallel/test-crypto-from-binary.js"),
    shared_official_batch_case!("test/parallel/test-crypto-secret-keygen.js"),
    shared_official_batch_case!("test/parallel/test-crypto-encoding-validation-error.js"),
    shared_official_batch_case!("test/parallel/test-crypto-hmac.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hash.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-getcipherinfo.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-oneshot-hash.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-random.js"),
    shared_official_batch_case!("test/parallel/test-crypto-randomfillsync-regression.js"),
    shared_official_batch_case!("test/parallel/test-crypto-randomuuid.js"),
    shared_official_batch_case!("test/parallel/test-crypto-update-encoding.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-cipheriv-decipheriv.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding-aes256.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-explicit-short-tag.js"),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-implicit-short-tag.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-classes.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-lazy-transform-writable.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-stream.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hkdf.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-pbkdf2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-scrypt.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-scrypt.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-scrypt.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-constructor.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-errors.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-leak.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-generate-keys.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-group-setters.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2-views.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-odd-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-shared.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-dh.js"),
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-ecdh-convert-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-default-shake-lengths.js",
        node20_fixture_source_path: Some(
            "node20/test/parallel/test-crypto-default-shake-lengths.js",
        ),
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-crypto-default-shake-lengths.js",
        ),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        ),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-oneshot-hash-xof.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-oneshot-hash-xof.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-v8-version-tag.js"),
    shared_official_batch_case!("test/parallel/test-v8-deserialize-buffer.js"),
    shared_official_batch_case!("test/parallel/test-v8-serdes.js"),
    shared_official_batch_case!("test/parallel/test-v8-stats.js"),
    shared_official_batch_case!("test/parallel/test-v8-flag-type-check.js"),
    shared_official_batch_case!("test/parallel/test-vm-basic.js"),
    shared_official_batch_case!("test/parallel/test-vm-context.js"),
    shared_official_batch_case!("test/parallel/test-vm-run-in-new-context.js"),
    shared_official_batch_case!("test/parallel/test-vm-strict-mode.js"),
    shared_official_batch_case!("test/parallel/test-vm-not-strict.js"),
    shared_official_batch_case!("test/parallel/test-vm-create-context-arg.js"),
    shared_official_batch_case!("test/parallel/test-inspector-module.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-inspector-invalid-args.js",
        INSPECTOR_FRONT_EDGE_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-inspector-open.js",
        INSPECTOR_FRONT_EDGE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-inspector-open-port-integer-overflow.js"),
    shared_official_batch_case!("test/parallel/test-inspector-enabled.js"),
];

// Keep only explicit targeted repros below. Green corpus coverage lives in the
// two manifest-driven batch lanes so the full suite does not execute the same
// fixture bodies twice.

