#[test]
#[ignore = "Pinned application-preset restriction: process.env string-key mutation and deletion are intentionally denied outside tooling-owned host surfaces"]
fn node22_process_env_delete_application_preset_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-env-delete.js",
        "node22/test/parallel/test-process-env-delete.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned application-preset restriction: process.env string-key mutation and deletion are intentionally denied outside tooling-owned host surfaces"]
fn node20_process_env_delete_application_preset_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-env-delete.js",
        "node20/test/parallel/test-process-env-delete.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned official Node20/Node22 assert gap: current runtime still disagrees with the shared test-assert-deep.js circular/deep-diff expectations"]
fn node22_assert_deep_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-assert-deep.js",
        "test/parallel/test-assert-deep.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned official Node20/Node22 assert gap: current runtime still disagrees with the shared test-assert-deep.js circular/deep-diff expectations"]
fn node20_assert_deep_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-assert-deep.js",
        "node20/test/parallel/test-assert-deep.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node22 runtime gap: official test-assert-partial-deep-equal.js currently aborts through a rusty_v8 weak-handle panic in the embedded runtime path"]
fn node22_assert_partial_deep_equal_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-assert-partial-deep-equal.js",
        "test/parallel/test-assert-partial-deep-equal.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared Deno-family inspect gap: revoked proxy formatting still throws inside ext/web and blocks test-console-issue-43095.js"]
fn node22_console_issue_43095_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-console-issue-43095.js",
        "test/parallel/test-console-issue-43095.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared Deno-family inspect gap: revoked proxy formatting still throws inside ext/web and blocks test-console-issue-43095.js"]
fn node20_console_issue_43095_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-console-issue-43095.js",
        "node20/test/parallel/test-console-issue-43095.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 still accepts once(emitter, event, null), while the current runtime matches the newer Node22 invalid-options behavior and rejects null"]
fn node20_events_once_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-events-once.js",
        "node20/test/parallel/test-events-once.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 process.features does not expose the Node22-only `typescript` key that Nimbus intentionally keeps in its single Node22-shaped runtime contract"]
fn node20_process_features_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-features.js",
        "node20/test/parallel/test-process-features.js",
        &[],
    );
}

const WHATWG_WEB_PLATFORM_COMMON_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures", "test/fixtures/wpt"];

const WHATWG_WEB_PLATFORM_ENCODING_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/encoding"];

const WHATWG_WEB_PLATFORM_LOW_ROI_PATHS: &[&str] = &[
    "test/parallel/test-whatwg-encoding-encodeinto-large.js",
    "test/parallel/test-whatwg-webstreams-transform-stream-members.js",
];

const WHATWG_WEB_PLATFORM_ENCODING_SIDE_BATCH_PATHS: &[&str] =
    &["test/parallel/test-whatwg-encoding-singlebyte.mjs"];

const WHATWG_WEB_PLATFORM_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-whatwg-encoding-custom-api-basics.js",
    "test/parallel/test-whatwg-encoding-custom-textdecoder-ignorebom.js",
    "test/parallel/test-whatwg-encoding-custom-textdecoder-streaming.js",
    "test/parallel/test-whatwg-events-add-event-listener-options-passive.js",
    "test/parallel/test-whatwg-events-customevent.js",
    "test/parallel/test-whatwg-events-event-constructors.js",
    "test/parallel/test-whatwg-events-eventtarget-this-of-listener.js",
    "test/parallel/test-whatwg-readablebytestreambyob.js",
    "test/parallel/test-whatwg-readablestream.mjs",
    "test/parallel/test-whatwg-url-custom-deepequal.js",
    "test/parallel/test-whatwg-url-custom-global.js",
    "test/parallel/test-whatwg-url-custom-href-side-effect.js",
    "test/parallel/test-whatwg-url-custom-searchparams.js",
    "test/parallel/test-whatwg-url-custom-searchparams-append.js",
    "test/parallel/test-whatwg-url-custom-searchparams-constructor.js",
    "test/parallel/test-whatwg-url-custom-searchparams-delete.js",
    "test/parallel/test-whatwg-url-custom-searchparams-entries.js",
    "test/parallel/test-whatwg-url-custom-searchparams-foreach.js",
    "test/parallel/test-whatwg-url-custom-searchparams-get.js",
    "test/parallel/test-whatwg-url-custom-searchparams-getall.js",
    "test/parallel/test-whatwg-url-custom-searchparams-has.js",
    "test/parallel/test-whatwg-url-custom-searchparams-inspect.js",
    "test/parallel/test-whatwg-url-custom-searchparams-keys.js",
    "test/parallel/test-whatwg-url-custom-searchparams-set.js",
    "test/parallel/test-whatwg-url-custom-searchparams-sort.js",
    "test/parallel/test-whatwg-url-custom-searchparams-stringifier.js",
    "test/parallel/test-whatwg-url-custom-searchparams-values.js",
    "test/parallel/test-whatwg-url-custom-setters.js",
    "test/parallel/test-whatwg-url-custom-tostringtag.js",
    "test/parallel/test-whatwg-url-override-hostname.js",
    "test/parallel/test-whatwg-url-properties.js",
    "test/parallel/test-whatwg-webstreams-adapters-to-readablestream.js",
    "test/parallel/test-whatwg-writablestream-close.js",
];

const WHATWG_WEB_PLATFORM_PROMOTED_NODE24_COMMON_EXTRA_PATHS: &[&str] =
    &["test/parallel/test-whatwg-transformstream-cancel-write-race.js"];

fn whatwg_web_platform_common_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths =
        node_compat_required_gap_paths_for_owner(lane, "node-compat/unpromoted-surface");
    fixture_paths.retain(|path| {
        path.starts_with("test/parallel/test-whatwg-")
            && !WHATWG_WEB_PLATFORM_LOW_ROI_PATHS
                .iter()
                .any(|low_roi_path| path == low_roi_path)
            && !WHATWG_WEB_PLATFORM_ENCODING_SIDE_BATCH_PATHS
                .iter()
                .any(|encoding_path| path == encoding_path)
    });
    fixture_paths
}

fn whatwg_web_platform_encoding_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths =
        node_compat_required_gap_paths_for_owner(lane, "node-compat/unpromoted-surface");
    fixture_paths.retain(|path| {
        WHATWG_WEB_PLATFORM_ENCODING_SIDE_BATCH_PATHS
            .iter()
            .any(|encoding_path| path == encoding_path)
    });
    fixture_paths
}

#[test]
fn node22_supported_lane_executes_whatwg_web_platform_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = WHATWG_WEB_PLATFORM_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-whatwg-web-platform-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        WHATWG_WEB_PLATFORM_COMMON_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_whatwg_web_platform_promoted_batch_fixture() {
    let mut fixture_paths: Vec<String> = WHATWG_WEB_PLATFORM_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    fixture_paths.extend(
        WHATWG_WEB_PLATFORM_PROMOTED_NODE24_COMMON_EXTRA_PATHS
            .iter()
            .map(|path| (*path).to_string()),
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-whatwg-web-platform-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        WHATWG_WEB_PLATFORM_COMMON_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_whatwg_web_platform_encoding_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = WHATWG_WEB_PLATFORM_ENCODING_SIDE_BATCH_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-whatwg-web-platform-encoding-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        WHATWG_WEB_PLATFORM_ENCODING_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked WHATWG/web-platform common required-gap inventory; excludes stress/hang paths and the lane-specific encoding fixture side batch"]
fn node22_supported_lane_whatwg_web_platform_watchpoint() {
    let fixture_paths = whatwg_web_platform_common_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-whatwg-web-platform-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        WHATWG_WEB_PLATFORM_COMMON_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked WHATWG/web-platform common required-gap inventory; excludes stress/hang paths and the lane-specific encoding fixture side batch"]
fn node24_default_lane_whatwg_web_platform_watchpoint() {
    let fixture_paths = whatwg_web_platform_common_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-whatwg-web-platform-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        WHATWG_WEB_PLATFORM_COMMON_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 side batch: Node24 WHATWG single-byte encoding fixture uses the Node24-only test/fixtures/encoding tree"]
fn node24_default_lane_whatwg_encoding_watchpoint() {
    let fixture_paths = whatwg_web_platform_encoding_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-whatwg-encoding-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        WHATWG_WEB_PLATFORM_ENCODING_EXTRA_DIRS,
    );
}

#[test]
fn node22_process_finalization_close_fixture() {
    run_manifested_fixture_with_postlude(
        "test/fixtures/process/close.mjs",
        "test/fixtures/process/close.mjs",
        &[],
        r#"
  globalThis.process.emit("exit", globalThis.process.exitCode ?? 0);
"#,
    );
}

#[test]
fn node22_process_finalization_before_exit_fixture() {
    run_manifested_fixture_with_postlude(
        "test/fixtures/process/before-exit.mjs",
        "test/fixtures/process/before-exit.mjs",
        &[],
        r#"
  globalThis.process.emit("beforeExit", globalThis.process.exitCode ?? 0);
  await new Promise((resolve) => setTimeout(resolve, 125));
  globalThis.process.emit("beforeExit", globalThis.process.exitCode ?? 0);
  globalThis.process.emit("exit", globalThis.process.exitCode ?? 0);
"#,
    );
}

#[test]
fn node22_process_finalization_unregister_fixture() {
    run_manifested_fixture_with_postlude(
        "test/fixtures/process/unregister.mjs",
        "test/fixtures/process/unregister.mjs",
        &[],
        r#"
  globalThis.process.emit("exit", globalThis.process.exitCode ?? 0);
"#,
    );
}

#[test]
#[ignore = "Pinned later-family dependency: official test-process-finalization.mjs now runs through the Nimbus sync subprocess harness, and the only remaining failure is different-registry-per-thread.mjs because worker_threads are still owned by a later compatibility family"]
fn node22_process_finalization_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-finalization.mjs",
        "node22/test/parallel/test-process-finalization.mjs",
        PROCESS_FINALIZATION_WATCHPOINT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 PerformanceResourceTiming#toJSON() omits the Node22-era `deliveryType` and `responseStatus` fields that Nimbus intentionally keeps in its single Node22-shaped runtime contract"]
fn node20_perf_hooks_resourcetiming_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-perf-hooks-resourcetiming.js",
        "node20/test/parallel/test-perf-hooks-resourcetiming.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-stream-duplex-readable-end.js still probes the older default-highWaterMark flow-control path, while the current runtime matches the later Node22/Node24 explicit-highWaterMark shape"]
fn node20_stream_duplex_readable_end_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-duplex-readable-end.js",
        "node20/test/parallel/test-stream-duplex-readable-end.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-stream-transform-split-highwatermark.js still expects the older 16 KiB split Transform default highWaterMark, while the current runtime matches the later Node22/Node24 getDefaultHighWaterMark() contract"]
fn node20_stream_transform_split_highwatermark_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-transform-split-highwatermark.js",
        "node20/test/parallel/test-stream-transform-split-highwatermark.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-stream-transform-split-objectmode.js still expects the older 16 KiB split Transform default highWaterMark, while the current runtime matches the later Node22/Node24 non-Windows 64 KiB contract"]
fn node20_stream_transform_split_objectmode_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-transform-split-objectmode.js",
        "node20/test/parallel/test-stream-transform-split-objectmode.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-stream-readable-infinite-read.js still depends on the older default Readable highWaterMark accumulation path, while the current runtime matches the later Node22/Node24 explicit-highWaterMark behavior"]
fn node20_stream_readable_infinite_read_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-infinite-read.js",
        "node20/test/parallel/test-stream-readable-infinite-read.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned application-preset path-policy divergence: test-fs-open.js expects ENOENT for an absolute missing host path outside the generated bundle root, while Nimbus intentionally denies that path before raw host open"]
fn node22_fs_open_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-open.js",
        "node22/test/parallel/test-fs-open.js",
        &[],
    );
}

#[test]
fn node22_fs_write_file_flush_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-file-flush.js",
        "node22/test/parallel/test-fs-write-file-flush.js",
        &[],
    );
}

#[test]
fn node22_fs_append_file_flush_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-append-file-flush.js",
        "node22/test/parallel/test-fs-append-file-flush.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_abort_signal_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-abort-signal.js",
        "node22/test/parallel/test-fs-watch-abort-signal.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES,
    );
}

#[test]
fn node22_fs_watch_enoent_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-enoent.js",
        "node22/test/parallel/test-fs-watch-enoent.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_promise_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-promise.js",
        "node22/test/parallel/test-fs-watch-recursive-promise.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_add_file_to_new_folder_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-add-file-to-new-folder.js",
        "node22/test/parallel/test-fs-watch-recursive-add-file-to-new-folder.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_add_file_to_existing_subfolder_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-add-file-to-existing-subfolder.js",
        "node22/test/parallel/test-fs-watch-recursive-add-file-to-existing-subfolder.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_watch_file_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-watch-file.js",
        "node22/test/parallel/test-fs-watch-recursive-watch-file.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_symlink_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-symlink.js",
        "node22/test/parallel/test-fs-watch-recursive-symlink.js",
        &[],
    );
}

#[test]
fn node22_fs_promises_watch_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-promises-watch.js",
        "node22/test/parallel/test-fs-promises-watch.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned application-preset divergence: test-fs-readdir-buffer.js probes /dev outside the generated bundle root, so the runtime intentionally denies that host path instead of claiming broad host-fs parity"]
fn node22_fs_readdir_buffer_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-readdir-buffer.js",
        "node22/test/parallel/test-fs-readdir-buffer.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned application-preset divergence: official test-fs-filehandle-use-after-close.js reopens process.execPath outside the generated bundle root, so the runtime intentionally denies that absolute host path before the later EBADF assertion can run"]
fn node22_fs_filehandle_use_after_close_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-filehandle-use-after-close.js",
        "node22/test/parallel/test-fs-filehandle-use-after-close.js",
        &[],
    );
}

#[test]
#[ignore = "Cross-family follow-up: official test-fs-write-file-sync.js no longer self-skips after the main-thread worker bootstrap fix and is green in the focused worker batch, but it has not been re-promoted into the streams/local-io denominator yet"]
fn node22_fs_write_file_sync_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-file-sync.js",
        "node22/test/parallel/test-fs-write-file-sync.js",
        &[],
    );
}

#[test]
#[ignore = "Cross-family loader-context follow-up seam: official test-fs-realpath.js no longer self-skips after the main-thread worker bootstrap fix and now fails on a real AlreadyExists symlink/setup path"]
fn node22_fs_realpath_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-realpath.js",
        "node22/test/parallel/test-fs-realpath.js",
        CYCLE_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node20_tty_backwards_api_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-backwards-api.js",
        "node20/test/parallel/test-tty-backwards-api.js",
        &[],
    );
}

#[test]
fn node22_tty_backwards_api_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-backwards-api.js",
        "node22/test/parallel/test-tty-backwards-api.js",
        &[],
    );
}

#[test]
fn node22_tty_stdin_end_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-end.js",
        "node22/test/parallel/test-tty-stdin-end.js",
        &[],
    );
}

#[test]
fn node22_tty_stdin_pipe_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-pipe.js",
        "node22/test/parallel/test-tty-stdin-pipe.js",
        &[],
    );
}

#[test]
fn node20_tty_stdin_end_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-end.js",
        "node20/test/parallel/test-tty-stdin-end.js",
        &[],
    );
}

#[test]
fn node20_tty_stdin_pipe_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-pipe.js",
        "node20/test/parallel/test-tty-stdin-pipe.js",
        &[],
    );
}

#[test]
fn node24_tty_stdin_end_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-end.js",
        "node24/test/parallel/test-tty-stdin-end.js",
        &[],
    );
}

#[test]
fn node24_tty_stdin_pipe_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-pipe.js",
        "node24/test/parallel/test-tty-stdin-pipe.js",
        &[],
    );
}

#[test]
fn node24_tty_backwards_api_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-backwards-api.js",
        "node24/test/parallel/test-tty-backwards-api.js",
        &[],
    );
}

#[test]
fn node24_console_clear_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-console-clear.js",
        "test/parallel/test-console-clear.js",
        &[],
    );
}

#[test]
fn node24_console_methods_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-console-methods.js",
        "test/parallel/test-console-methods.js",
        &[],
    );
}

#[test]
fn node24_events_add_abort_listener_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-events-add-abort-listener.mjs",
        "node24/test/parallel/test-events-add-abort-listener.mjs",
        NODE24_COMMON_INDEX_MJS_EXTRA_FILES,
    );
}

#[test]
fn node24_url_parse_format_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-url-parse-format.js",
        "node24/test/parallel/test-url-parse-format.js",
        &[],
    );
}

#[test]
fn node24_url_parse_invalid_input_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-url-parse-invalid-input.js",
        "node24/test/parallel/test-url-parse-invalid-input.js",
        &[],
    );
}

#[test]
fn node24_url_path_to_file_url_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-url-pathtofileurl.js",
        "node24/test/parallel/test-url-pathtofileurl.js",
        &[],
    );
}

#[test]
fn node22_readline_csi_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-csi.js",
        "node22/test/parallel/test-readline-csi.js",
        &[],
    );
}

#[test]
fn node22_readline_carriage_return_between_chunks_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-carriage-return-between-chunks.js",
        "node22/test/parallel/test-readline-carriage-return-between-chunks.js",
        &[],
    );
}

#[test]
fn node22_readline_input_onerror_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-input-onerror.js",
        "node22/test/parallel/test-readline-input-onerror.js",
        &[],
    );
}

#[test]
fn node22_readline_promises_csi_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-promises-csi.mjs",
        "node22/test/parallel/test-readline-promises-csi.mjs",
        NODE22_COMMON_INDEX_MJS_EXTRA_FILES,
    );
}

#[test]
fn node22_stream_add_abort_signal_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-add-abort-signal.js",
        "node22/test/parallel/test-stream-add-abort-signal.js",
        &[],
    );
}

#[test]
fn node22_stream_base_prototype_accessors_enumerability_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-base-prototype-accessors-enumerability.js",
        "node22/test/parallel/test-stream-base-prototype-accessors-enumerability.js",
        &[],
    );
}

#[test]
fn node22_stream_catch_rejections_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-catch-rejections.js",
        "node22/test/parallel/test-stream-catch-rejections.js",
        &[],
    );
}

#[test]
fn node22_stream_compose_operator_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-compose-operator.js",
        "node22/test/parallel/test-stream-compose-operator.js",
        &[],
    );
}

#[test]
fn node22_stream_set_default_hwm_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-set-default-hwm.js",
        "node22/test/parallel/test-stream-set-default-hwm.js",
        &[],
    );
}

#[test]
fn node22_stream_readable_dispose_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-dispose.js",
        "node22/test/parallel/test-stream-readable-dispose.js",
        &[],
    );
}

#[test]
fn node22_stream_readable_from_web_termination_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-from-web-termination.js",
        "node22/test/parallel/test-stream-readable-from-web-termination.js",
        &[],
    );
}

#[test]
fn node22_stream_readable_strategy_option_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-strategy-option.js",
        "node22/test/parallel/test-stream-readable-strategy-option.js",
        &[],
    );
}

#[test]
fn node22_stream_readable_to_web_termination_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-to-web-termination.js",
        "node22/test/parallel/test-stream-readable-to-web-termination.js",
        &[],
    );
}

#[test]
fn node22_stream_state_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-stream-state-batch",
        "node22",
        NODE22_STREAM_STATE_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_stream_buffering_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-stream-buffering-batch",
        "node22",
        NODE22_STREAM_BUFFERING_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_tty_os_tail_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-tty-os-tail-batch",
        "node22",
        NODE22_TTY_OS_TAIL_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_pure_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-pure-batch",
        "node22",
        NODE22_NETWORKING_PURE_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_net_server_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-net-server-batch",
        "node22",
        NODE22_NETWORKING_NET_SERVER_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_net_socket_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-net-socket-batch",
        "node22",
        NODE22_NETWORKING_NET_SOCKET_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_request_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-request-batch",
        "node22",
        NODE22_NETWORKING_HTTP_REQUEST_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_timeout_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-timeout-batch",
        "node22",
        NODE22_NETWORKING_HTTP_TIMEOUT_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_response_positive_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-response-positive-batch",
        "node22",
        NODE22_NETWORKING_HTTP_RESPONSE_POSITIVE_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_response_state_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-response-state-batch",
        "node22",
        NODE22_NETWORKING_HTTP_RESPONSE_STATE_BATCH_FIXTURES,
        &[],
    );
    run_node_compat_watchpoint_batch(
        "node22-networking-http-response-state-countdown-batch",
        "node22",
        NODE22_NETWORKING_HTTP_RESPONSE_STATE_COUNTDOWN_BATCH_FIXTURES,
        COMMON_COUNTDOWN_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_server_no_arg_listen_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-server-no-arg-listen-batch",
        "node22",
        NODE22_NETWORKING_SERVER_NO_ARG_LISTEN_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_agent_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-agent-batch",
        "node22",
        NODE22_NETWORKING_HTTP_AGENT_BATCH_FIXTURES,
        COMMON_COUNTDOWN_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_http_agent_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-agent-lifecycle-batch",
        "node22",
        NODE22_NETWORKING_HTTP_AGENT_LIFECYCLE_BATCH_FIXTURES,
        COMMON_COUNTDOWN_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_dgram_helper_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-helper-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_HELPER_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_dgram_bind_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-bind-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_BIND_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_dgram_connect_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-connect-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_CONNECT_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_dgram_send_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-send-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_SEND_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_dgram_remaining_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-remaining-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_REMAINING_BATCH_FIXTURES,
        NODE22_COMMON_UDP_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_dgram_internal_gap_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-internal-gap",
        "node22",
        NODE22_NETWORKING_DGRAM_INTERNAL_GAP_FIXTURES,
        NODE22_COMMON_UDP_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_dgram_local_patch_regression_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-local-patch-regression-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_LOCAL_PATCH_REGRESSION_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_crypto_gated_helper_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-crypto-gated-helper-batch",
        "node22",
        NODE22_NETWORKING_CRYPTO_GATED_HELPER_BATCH_FIXTURES,
        COMMON_TLS_KEY_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Deno TLS/http2 helper gap: official HTTPS certificate verification and http2 packed-settings buffer internals diverge from Node in the embedded lane"]
fn node22_networking_crypto_gated_helper_gap_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-crypto-gated-helper-gap",
        "node22",
        NODE22_NETWORKING_CRYPTO_GATED_HELPER_GAP_FIXTURES,
        COMMON_TLS_KEY_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_http2_header_status_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-header-status-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_HEADER_STATUS_BATCH,
    );
}

#[test]
fn node22_networking_http2_compat_request_response_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-compat-request-response-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_BATCH,
    );
}

#[test]
#[ignore = "Pinned Deno http2 compat gap: Http2ServerRequest.trailers currently reports an empty object where Node expects null before trailers arrive"]
fn node22_networking_http2_compat_request_response_gap_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-compat-request-response-gap",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_GAP_BATCH,
    );
}

#[test]
fn node22_networking_http2_compat_serverresponse_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-compat-serverresponse-lifecycle-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_COMPAT_SERVERRESPONSE_LIFECYCLE_BATCH,
    );
}

#[test]
fn node22_networking_http2_compat_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-compat-remainder-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_COMPAT_REMAINDER_BATCH,
    );
}

#[test]
fn node22_networking_https_agent_session_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-agent-session-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_AGENT_SESSION_BATCH,
    );
}

#[test]
#[ignore = "Pinned Deno HTTPS agent/session gap: SNI propagation and globalAgent socket bookkeeping do not yet match Node's agent internals"]
fn node22_networking_https_agent_session_gap_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-agent-session-gap",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_AGENT_SESSION_GAP_BATCH,
    );
}

#[test]
fn node22_networking_https_local_server_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-local-server-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_LOCAL_SERVER_BATCH,
    );
}

#[test]
fn node22_networking_https_server_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-server-lifecycle-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_SERVER_LIFECYCLE_BATCH,
    );
}

#[test]
fn node22_networking_https_client_server_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-client-server-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_CLIENT_SERVER_BATCH,
    );
}

const NODE22_NETWORKING_HTTPS_TLS_SESSION_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "node22-networking-https-tls-session-batch",
        "node-compat-networking",
        "runs Node22 HTTPS TLS/session fixtures in a subprocess because TLS session reuse is timing-sensitive under the parallel Rust test harness",
        "runtime::tests::node_compat::node22_networking_https_tls_session_batch_fixture_subprocess",
    );

#[test]
fn node22_networking_https_tls_session_batch_fixture() {
    run_v8_sensitive_runtime_test_in_subprocess(NODE22_NETWORKING_HTTPS_TLS_SESSION_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate TLS session reuse from parallel runtime fixture pressure"]
fn node22_networking_https_tls_session_batch_fixture_subprocess() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-tls-session-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_TLS_SESSION_BATCH,
    );
}

#[test]
#[ignore = "Pinned Deno TLS session gap: Node's enableTicketKeyCallback renewal hook is not exposed by the embedded TLS context"]
fn node22_networking_https_tls_session_gap_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-tls-session-gap",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_TLS_SESSION_GAP_BATCH,
    );
}

#[test]
fn node22_networking_tls_local_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-tls-local-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_TLS_LOCAL_BATCH,
    );
}

#[test]
fn node22_loader_context_module_commonjs_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-module-commonjs-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_MODULE_COMMONJS_BATCH,
    );
}

#[test]
fn node22_loader_context_async_local_storage_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-async-local-storage-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ASYNC_LOCAL_STORAGE_BATCH,
    );
}

#[test]
fn node24_loader_context_async_local_storage_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-async-local-storage-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ASYNC_LOCAL_STORAGE_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node20 legacy-lane divergence: official v20.20.2 test-async-local-storage-exit-does-not-leak.js still expects the old JavaScript AsyncLocalStorage _propagate hook, while the current runtime matches the newer Node22/Node24 implementation shape"]
fn node20_async_local_storage_exit_does_not_leak_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-async-local-storage-exit-does-not-leak.js",
        "node20/test/parallel/test-async-local-storage-exit-does-not-leak.js",
        &[],
    );
}

#[test]
fn node22_loader_context_async_hooks_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-async-hooks-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_BATCH,
    );
}

#[test]
fn node20_loader_context_async_hooks_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-async-hooks-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_BATCH,
    );
}

#[test]
fn node24_loader_context_async_hooks_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-async-hooks-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks AsyncResource gap: Deno internal/async_hooks.ts currently lacks Node-compatible AsyncResource validation, triggerAsyncId(), and selected init hook emissions"]
fn node22_loader_context_async_hooks_resource_gap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-async-hooks-resource-gap-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_RESOURCE_GAP_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks AsyncResource gap: Deno internal/async_hooks.ts currently lacks Node-compatible AsyncResource validation, triggerAsyncId(), and selected init hook emissions"]
fn node20_loader_context_async_hooks_resource_gap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-async-hooks-resource-gap-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_RESOURCE_GAP_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks AsyncResource gap: Deno internal/async_hooks.ts currently lacks Node-compatible AsyncResource validation, triggerAsyncId(), and selected init hook emissions"]
fn node24_loader_context_async_hooks_resource_gap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-async-hooks-resource-gap-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_RESOURCE_GAP_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks promise gap: Deno internal/async_hooks.ts does not currently enable V8 promise hooks, so promise/await init events are not emitted"]
fn node22_loader_context_async_hooks_promise_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-async-hooks-promise-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks promise gap: Deno internal/async_hooks.ts does not currently enable V8 promise hooks, so promise/await init events are not emitted"]
fn node20_loader_context_async_hooks_promise_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-async-hooks-promise-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks promise gap: Deno internal/async_hooks.ts does not currently enable V8 promise hooks, so promise/await init events are not emitted"]
fn node24_loader_context_async_hooks_promise_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-async-hooks-promise-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks promise gap: Deno internal/async_hooks.ts does not currently enable V8 promise hooks, so promise/await init events are not emitted"]
fn node22_loader_context_async_hooks_promise_core_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-async-hooks-promise-core-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_CORE_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks promise gap: Deno internal/async_hooks.ts does not currently enable V8 promise hooks, so promise/await init events are not emitted"]
fn node20_loader_context_async_hooks_promise_core_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-async-hooks-promise-core-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_CORE_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node async_hooks promise gap: Deno internal/async_hooks.ts does not currently enable V8 promise hooks, so promise/await init events are not emitted"]
fn node24_loader_context_async_hooks_promise_core_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-async-hooks-promise-core-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_CORE_BATCH,
    );
}
