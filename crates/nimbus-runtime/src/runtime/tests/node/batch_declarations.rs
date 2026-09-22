// The inventory of every declared node_compat batch, and the invariants that
// keep one fixture's staging the same wherever it is declared.
//
// A fixture runs under more than one batch on purpose: a family batch measures
// the whole family, and a watchpoint batch measures a focused neighbourhood.
// That is only sound while every batch stages the fixture the same way. When
// two batches disagreed, the same fixture passed under one entry point and
// failed under the other, and the corpus baseline recorded whichever one the
// measurement happened to reach.
//
// The agreement is per lane, because two batches may deliberately split the
// lanes of one fixture between them. A batch that does not run the fixture in
// a lane makes no claim about that lane.

const NODE_COMPAT_DECLARED_BATCHES: &[(&str, &[NodeCompatBatchEntry])] = &[
    ("CORE_SEMANTICS_BATCH", CORE_SEMANTICS_BATCH),
    ("FS_CP_BATCH", FS_CP_BATCH),
    ("LOADER_CONTEXT_BATCH", LOADER_CONTEXT_BATCH),
    ("LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH", LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH),
    ("LOADER_CONTEXT_CRYPTO_XOF_EXTENSION_BATCH", LOADER_CONTEXT_CRYPTO_XOF_EXTENSION_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_INSPECTOR_FRONT_EDGE_BATCH", LOADER_CONTEXT_FOLLOWUP_INSPECTOR_FRONT_EDGE_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH", LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_V8_GREEN_BATCH", LOADER_CONTEXT_FOLLOWUP_V8_GREEN_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_V8_HELPER_BATCH", LOADER_CONTEXT_FOLLOWUP_V8_HELPER_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_VM_BASIC_BATCH", LOADER_CONTEXT_FOLLOWUP_VM_BASIC_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_VM_CONTEXT_REGRESSION_BATCH", LOADER_CONTEXT_FOLLOWUP_VM_CONTEXT_REGRESSION_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_VM_CONTEXT_REMAINDER_REGRESSION_BATCH", LOADER_CONTEXT_FOLLOWUP_VM_CONTEXT_REMAINDER_REGRESSION_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_WORKER_BASIC_BATCH", LOADER_CONTEXT_FOLLOWUP_WORKER_BASIC_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_WORKER_BOOTSTRAP_BATCH", LOADER_CONTEXT_FOLLOWUP_WORKER_BOOTSTRAP_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_WORKER_CONTRACT_BATCH", LOADER_CONTEXT_FOLLOWUP_WORKER_CONTRACT_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_WORKER_MAIN_THREAD_BATCH", LOADER_CONTEXT_FOLLOWUP_WORKER_MAIN_THREAD_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_WORKER_MESSAGE_CHANNEL_BATCH", LOADER_CONTEXT_FOLLOWUP_WORKER_MESSAGE_CHANNEL_BATCH),
    ("LOADER_CONTEXT_FOLLOWUP_WORKER_MESSAGE_PORT_BATCH", LOADER_CONTEXT_FOLLOWUP_WORKER_MESSAGE_PORT_BATCH),
    ("LOADER_CONTEXT_SUPPLEMENTARY_BATCH", LOADER_CONTEXT_SUPPLEMENTARY_BATCH),
    ("LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH", LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH),
    ("LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH", LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH),
    ("NDS3_WAVE25_ESM_IMPORT_ATTRIBUTES_ERRORS_N22_BATCH", NDS3_WAVE25_ESM_IMPORT_ATTRIBUTES_ERRORS_N22_BATCH),
    ("NETWORKING_BATCH", NETWORKING_BATCH),
    ("NODE20_LOADER_CONTEXT_CRYPTO_AUTHENTICATED_SUPPORTED_WATCHPOINT_BATCH", NODE20_LOADER_CONTEXT_CRYPTO_AUTHENTICATED_SUPPORTED_WATCHPOINT_BATCH),
    ("NODE20_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH", NODE20_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH),
    ("NODE20_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH", NODE20_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH),
    ("NODE20_LOADER_CONTEXT_CRYPTO_DH_SUPPORTED_WATCHPOINT_BATCH", NODE20_LOADER_CONTEXT_CRYPTO_DH_SUPPORTED_WATCHPOINT_BATCH),
    ("NODE22_LOADER_CONTEXT_ASYNC_HOOKS_BATCH", NODE22_LOADER_CONTEXT_ASYNC_HOOKS_BATCH),
    ("NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_BATCH", NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_BATCH),
    ("NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_CORE_BATCH", NODE22_LOADER_CONTEXT_ASYNC_HOOKS_PROMISE_CORE_BATCH),
    ("NODE22_LOADER_CONTEXT_ASYNC_HOOKS_RESOURCE_GAP_BATCH", NODE22_LOADER_CONTEXT_ASYNC_HOOKS_RESOURCE_GAP_BATCH),
    ("NODE22_LOADER_CONTEXT_ASYNC_LOCAL_STORAGE_BATCH", NODE22_LOADER_CONTEXT_ASYNC_LOCAL_STORAGE_BATCH),
    ("NODE22_LOADER_CONTEXT_CRYPTO_CIPHER_AND_PADDING_BATCH", NODE22_LOADER_CONTEXT_CRYPTO_CIPHER_AND_PADDING_BATCH),
    ("NODE22_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH", NODE22_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH),
    ("NODE22_LOADER_CONTEXT_CRYPTO_DH_CURVES_AND_STATELESS_BATCH", NODE22_LOADER_CONTEXT_CRYPTO_DH_CURVES_AND_STATELESS_BATCH),
    ("NODE22_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH", NODE22_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH),
    ("NODE22_LOADER_CONTEXT_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH", NODE22_LOADER_CONTEXT_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH),
    ("NODE22_LOADER_CONTEXT_CRYPTO_KDF_AND_STREAM_BATCH", NODE22_LOADER_CONTEXT_CRYPTO_KDF_AND_STREAM_BATCH),
    ("NODE22_LOADER_CONTEXT_MODULE_COMMONJS_BATCH", NODE22_LOADER_CONTEXT_MODULE_COMMONJS_BATCH),
    ("NODE22_LOADER_CONTEXT_ZLIB_BROTLI_AND_CONTROL_BATCH", NODE22_LOADER_CONTEXT_ZLIB_BROTLI_AND_CONTROL_BATCH),
    ("NODE22_LOADER_CONTEXT_ZLIB_DECOMPRESSION_BATCH", NODE22_LOADER_CONTEXT_ZLIB_DECOMPRESSION_BATCH),
    ("NODE22_LOADER_CONTEXT_ZLIB_FOUNDATION_BATCH", NODE22_LOADER_CONTEXT_ZLIB_FOUNDATION_BATCH),
    ("NODE22_LOADER_CONTEXT_ZLIB_STREAM_LIFECYCLE_BATCH", NODE22_LOADER_CONTEXT_ZLIB_STREAM_LIFECYCLE_BATCH),
    ("NODE22_NETWORKING_HTTP2_COMPAT_REMAINDER_BATCH", NODE22_NETWORKING_HTTP2_COMPAT_REMAINDER_BATCH),
    ("NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_BATCH", NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_BATCH),
    ("NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_GAP_BATCH", NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_GAP_BATCH),
    ("NODE22_NETWORKING_HTTP2_COMPAT_SERVERRESPONSE_LIFECYCLE_BATCH", NODE22_NETWORKING_HTTP2_COMPAT_SERVERRESPONSE_LIFECYCLE_BATCH),
    ("NODE22_NETWORKING_HTTP2_HEADER_STATUS_BATCH", NODE22_NETWORKING_HTTP2_HEADER_STATUS_BATCH),
    ("NODE22_NETWORKING_HTTPS_AGENT_SESSION_BATCH", NODE22_NETWORKING_HTTPS_AGENT_SESSION_BATCH),
    ("NODE22_NETWORKING_HTTPS_AGENT_SESSION_GAP_BATCH", NODE22_NETWORKING_HTTPS_AGENT_SESSION_GAP_BATCH),
    ("NODE22_NETWORKING_HTTPS_CLIENT_SERVER_BATCH", NODE22_NETWORKING_HTTPS_CLIENT_SERVER_BATCH),
    ("NODE22_NETWORKING_HTTPS_LOCAL_SERVER_BATCH", NODE22_NETWORKING_HTTPS_LOCAL_SERVER_BATCH),
    ("NODE22_NETWORKING_HTTPS_SERVER_LIFECYCLE_BATCH", NODE22_NETWORKING_HTTPS_SERVER_LIFECYCLE_BATCH),
    ("NODE22_NETWORKING_HTTPS_TLS_SESSION_BATCH", NODE22_NETWORKING_HTTPS_TLS_SESSION_BATCH),
    ("NODE22_NETWORKING_HTTPS_TLS_SESSION_GAP_BATCH", NODE22_NETWORKING_HTTPS_TLS_SESSION_GAP_BATCH),
    ("NODE22_NETWORKING_TLS_LOCAL_BATCH", NODE22_NETWORKING_TLS_LOCAL_BATCH),
    ("NODE_TOOLS_CLUSTER_WORKER_FOUNDATION_BATCH", NODE_TOOLS_CLUSTER_WORKER_FOUNDATION_BATCH),
    ("NODE_TOOLS_CLUSTER_WORKER_LIFECYCLE_BATCH", NODE_TOOLS_CLUSTER_WORKER_LIFECYCLE_BATCH),
    ("NODE_TOOLS_CONSTANTS_FOUNDATION_BATCH", NODE_TOOLS_CONSTANTS_FOUNDATION_BATCH),
    ("NODE_TOOLS_DOMAIN_FOUNDATION_BATCH", NODE_TOOLS_DOMAIN_FOUNDATION_BATCH),
    ("NODE_TOOLS_REPL_FOUNDATION_BATCH", NODE_TOOLS_REPL_FOUNDATION_BATCH),
    ("NODE_TOOLS_SEA_FOUNDATION_BATCH", NODE_TOOLS_SEA_FOUNDATION_BATCH),
    ("NODE_TOOLS_SQLITE_FOUNDATION_BATCH", NODE_TOOLS_SQLITE_FOUNDATION_BATCH),
    ("NODE_TOOLS_SYS_FOUNDATION_BATCH", NODE_TOOLS_SYS_FOUNDATION_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_CLI_OPTIONS_BATCH", NODE_TOOLS_TEST_RUNNER_CLI_OPTIONS_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_CLI_RANDOMIZE_BATCH", NODE_TOOLS_TEST_RUNNER_CLI_RANDOMIZE_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_CLI_RERUN_FAILURES_BATCH", NODE_TOOLS_TEST_RUNNER_CLI_RERUN_FAILURES_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_CONTEXT_METADATA_BATCH", NODE_TOOLS_TEST_RUNNER_CONTEXT_METADATA_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_FOUNDATION_BATCH", NODE_TOOLS_TEST_RUNNER_FOUNDATION_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_OPTION_VALIDATION_BATCH", NODE_TOOLS_TEST_RUNNER_OPTION_VALIDATION_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_PLAN_BATCH", NODE_TOOLS_TEST_RUNNER_PLAN_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_REPORTERS_BATCH", NODE_TOOLS_TEST_RUNNER_REPORTERS_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_REPORTER_OUTPUT_BATCH", NODE_TOOLS_TEST_RUNNER_REPORTER_OUTPUT_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_RUN_EDGE_BATCH", NODE_TOOLS_TEST_RUNNER_RUN_EDGE_BATCH),
    ("NODE_TOOLS_TEST_RUNNER_RUN_EVENT_METADATA_BATCH", NODE_TOOLS_TEST_RUNNER_RUN_EVENT_METADATA_BATCH),
    ("NODE_TOOLS_TRACE_EVENTS_FOUNDATION_BATCH", NODE_TOOLS_TRACE_EVENTS_FOUNDATION_BATCH),
    ("NODE_TOOLS_WASI_EXECUTION_BATCH", NODE_TOOLS_WASI_EXECUTION_BATCH),
    ("NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_BATCH", NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_BATCH),
    ("NODE_TOOLS_WASI_IO_SUBCASE_WATCHPOINT_BATCH", NODE_TOOLS_WASI_IO_SUBCASE_WATCHPOINT_BATCH),
    ("NODE_TOOLS_WASI_PREOPEN_IO_BATCH", NODE_TOOLS_WASI_PREOPEN_IO_BATCH),
    ("NODE_TOOLS_WASI_VALIDATION_BATCH", NODE_TOOLS_WASI_VALIDATION_BATCH),
    ("PROCESS_AND_TIMING_BATCH", PROCESS_AND_TIMING_BATCH),
    ("PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH", PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH),
    ("RUNTIME_SUPPLEMENTARY_BATCH", RUNTIME_SUPPLEMENTARY_BATCH),
    ("RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH", RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH),
    ("STREAMS_AND_LOCAL_IO_BATCH", STREAMS_AND_LOCAL_IO_BATCH),
];

/// Groups every declaration of one fixture, keyed by its test-relative path.
fn node_compat_declarations_by_fixture()
-> std::collections::BTreeMap<&'static str, Vec<(&'static str, &'static NodeCompatBatchEntry)>> {
    let mut declarations: std::collections::BTreeMap<
        &'static str,
        Vec<(&'static str, &'static NodeCompatBatchEntry)>,
    > = std::collections::BTreeMap::new();
    for (batch_name, entries) in NODE_COMPAT_DECLARED_BATCHES {
        for entry in entries.iter() {
            declarations
                .entry(entry.test_relative_path)
                .or_default()
                .push((batch_name, entry));
        }
    }
    declarations
}

#[test]
fn node_compat_every_declared_batch_is_registered() {
    // The registry is what the invariants below read, so a batch that is
    // missing from it is a batch with no guard. Counting the declarations
    // catches a new batch const that nobody added here.
    assert_eq!(
        NODE_COMPAT_DECLARED_BATCHES.len(),
        86,
        "a batch const was added or removed without updating \
         NODE_COMPAT_DECLARED_BATCHES"
    );
    let mut names: Vec<&str> = NODE_COMPAT_DECLARED_BATCHES
        .iter()
        .map(|(name, _)| *name)
        .collect();
    names.sort_unstable();
    let registered = names.len();
    names.dedup();
    assert_eq!(registered, names.len(), "a batch is registered twice");
}

#[test]
fn node_compat_no_batch_declares_one_fixture_twice() {
    let mut offenders = Vec::new();
    for (batch_name, entries) in NODE_COMPAT_DECLARED_BATCHES {
        let mut seen = std::collections::BTreeSet::new();
        for entry in entries.iter() {
            if !seen.insert(entry.test_relative_path) {
                offenders.push(format!("{batch_name}: {}", entry.test_relative_path));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a batch runs the same fixture twice in one process, which measures \
         the second run against the state the first left behind:\n{}",
        offenders.join("\n")
    );
}

/// Every lane the corpus measures, so an invariant covers all of them.
const NODE_COMPAT_LANES: &[NodeCompatLane] = &[
    NodeCompatLane::Node20,
    NodeCompatLane::Node22,
    NodeCompatLane::Node24,
    NodeCompatLane::Node26,
];

#[test]
fn node_compat_one_fixture_is_staged_one_way_per_lane() {
    // Two batches may cover different lanes of one fixture, so the contract is
    // per lane: every batch that runs the fixture in a lane must stage it from
    // the same source with the same helpers. A batch that does not run it in
    // that lane has nothing to agree with.
    let mut offenders = Vec::new();
    for (test_relative_path, declarations) in node_compat_declarations_by_fixture() {
        for lane in NODE_COMPAT_LANES {
            let staged: Vec<(&str, Cow<'static, str>, &[NodeCompatExtraFixtureEntry])> =
                declarations
                    .iter()
                    .filter_map(|(batch_name, entry)| {
                        entry
                            .fixture_source_path_for_lane(*lane)
                            .map(|source| (*batch_name, source, entry.extra_files_for_lane(*lane)))
                    })
                    .collect();
            let Some((first_batch, first_source, first_extra)) = staged.first() else {
                continue;
            };
            for (batch_name, source, extra) in staged.iter().skip(1) {
                if source != first_source || extra != first_extra {
                    offenders.push(format!(
                        "{test_relative_path} on {}\n  {first_batch}: {first_source} {first_extra:?}\n  {batch_name}: {source} {extra:?}",
                        node_compat_lane_name(*lane)
                    ));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these fixtures are staged differently depending on which batch runs \
         them, so their measurement depends on the entry point rather than on \
         the runtime:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn node_compat_every_lane_of_one_fixture_is_given_the_same_helpers() {
    // A batch entry names its extra helper files per lane, and `extra_files_for
    // _lane` reads the lane list when it has one and the shared list otherwise.
    // Node 26 has no lane list at all, so an entry that named helpers only for
    // the older lanes handed node26 an empty bundle, and the fixture died on a
    // missing helper that every other lane received. The corpus then recorded
    // that staging hole as a Nimbus gap.
    //
    // The contract is therefore a parity one: whatever lanes an entry runs, they
    // are all given the same helpers by runtime path. A lane may resolve a
    // helper from its own vendored copy, but no lane may be left without one.
    let mut offenders = Vec::new();
    for (test_relative_path, declarations) in node_compat_declarations_by_fixture() {
        for (batch_name, entry) in declarations {
            let staged: Vec<(NodeCompatLane, Vec<&str>)> = NODE_COMPAT_LANES
                .iter()
                .filter(|lane| entry.fixture_source_path_for_lane(**lane).is_some())
                .map(|lane| {
                    let mut runtime_paths: Vec<&str> = entry
                        .extra_files_for_lane(*lane)
                        .iter()
                        .map(|extra| extra.runtime_path)
                        .collect();
                    runtime_paths.sort_unstable();
                    (*lane, runtime_paths)
                })
                .collect();
            let Some((first_lane, first_paths)) = staged.first() else {
                continue;
            };
            for (lane, runtime_paths) in staged.iter().skip(1) {
                if runtime_paths != first_paths {
                    offenders.push(format!(
                        "`{test_relative_path}` in `{batch_name}`: {} stages {first_paths:?} but {} stages {runtime_paths:?}",
                        node_compat_lane_name(*first_lane),
                        node_compat_lane_name(*lane),
                    ));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these fixtures leave a lane without helpers the other lanes receive:\n  {}",
        offenders.join("\n  "),
    );
}

#[test]
fn node_compat_no_lane_is_staged_from_another_lanes_tree() {
    // A helper whose source path names no lane is resolved lane first, so each
    // lane reads its own vendored copy when it has one. A helper whose source
    // path names a lane is read verbatim, so declaring one in a list that more
    // than one lane reads stages that lane's copy into every other lane. That
    // is how a node20 helper reached a node26 bundle: the shared list is the
    // fallback for every lane, and node26 has no list of its own.
    let mut offenders = Vec::new();
    for (test_relative_path, declarations) in node_compat_declarations_by_fixture() {
        for (batch_name, entry) in declarations {
            for lane in NODE_COMPAT_LANES {
                if entry.fixture_source_path_for_lane(*lane).is_none() {
                    continue;
                }
                for extra in entry.extra_files_for_lane(*lane) {
                    let source_lane =
                        inferred_node_compat_lane_from_fixture_source_path(extra.fixture_source_path);
                    if matches!(source_lane, Some(source_lane) if source_lane != *lane) {
                        offenders.push(format!(
                            "`{test_relative_path}` in `{batch_name}`: {} stages `{}` from `{}`",
                            node_compat_lane_name(*lane),
                            extra.runtime_path,
                            extra.fixture_source_path,
                        ));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these fixtures stage one lane's helper into another lane:\n  {}",
        offenders.join("\n  "),
    );
}

#[test]
fn node_compat_every_staged_helper_resolves_in_every_lane_that_stages_it() {
    // A helper is now named once, without a lane, and each lane reads its own
    // vendored copy when it has one and the shared copy otherwise. That makes a
    // lane with neither a hard error deep inside a fixture run, where it reads
    // as a Nimbus gap rather than as the missing fixture file it is. Resolve
    // every declared helper here instead, so a gap in the vendored trees fails
    // as a staging error with the lane and the path named.
    let mut offenders = Vec::new();
    for (test_relative_path, declarations) in node_compat_declarations_by_fixture() {
        for (batch_name, entry) in declarations {
            for lane in NODE_COMPAT_LANES {
                if entry.fixture_source_path_for_lane(*lane).is_none() {
                    continue;
                }
                for extra in entry.extra_files_for_lane(*lane) {
                    if node_compat_helper_is_synthetic(extra.runtime_path) {
                        continue;
                    }
                    let lane_source = node_compat_fixture_root()
                        .join(node_compat_lane_name(*lane))
                        .join(extra.fixture_source_path);
                    let shared_source =
                        node_compat_fixture_root().join(extra.fixture_source_path);
                    if !lane_source.is_file() && !shared_source.is_file() {
                        offenders.push(format!(
                            "`{test_relative_path}` in `{batch_name}`: {} has no copy of `{}`",
                            node_compat_lane_name(*lane),
                            extra.fixture_source_path,
                        ));
                    }
                }
            }
        }
    }
    offenders.sort_unstable();
    offenders.dedup();
    assert!(
        offenders.is_empty(),
        "these staged helpers resolve in no tree the lane can read:\n  {}",
        offenders.join("\n  "),
    );
}
