use self::supplementary_batches::{
    LOADER_CONTEXT_SUPPLEMENTARY_BATCH, LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH,
    LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH, PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH,
    RUNTIME_SUPPLEMENTARY_BATCH, RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH,
};

fn snapshot_batch_entries(batch: &[NodeCompatBatchEntry]) -> Vec<NodeCompatBatchEntrySnapshot> {
    batch
        .iter()
        .map(|entry| NodeCompatBatchEntrySnapshot {
            test_relative_path: entry.test_relative_path,
            node20_fixture_source_path: entry.node20_fixture_source_path,
            node22_fixture_source_path: entry.node22_fixture_source_path,
            node24_fixture_source_path: entry.node24_fixture_source_path,
        })
        .collect()
}

pub(super) fn core_semantics_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(CORE_SEMANTICS_BATCH)
}

pub(super) fn process_and_timing_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(PROCESS_AND_TIMING_BATCH)
}

pub(super) fn streams_and_local_io_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(STREAMS_AND_LOCAL_IO_BATCH)
}

pub(super) fn networking_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(NETWORKING_BATCH)
}

pub(super) fn loader_context_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(LOADER_CONTEXT_BATCH)
}

pub(super) fn loader_context_supplementary_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(LOADER_CONTEXT_SUPPLEMENTARY_BATCH)
}

pub(super) fn loader_context_supplementary_module_bridge_batch_snapshot()
-> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH)
}

pub(super) fn loader_context_supplementary_global_injection_batch_snapshot()
-> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH)
}

pub(super) fn process_and_timing_supplementary_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot>
{
    snapshot_batch_entries(PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH)
}

pub(super) fn runtime_supplementary_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(RUNTIME_SUPPLEMENTARY_BATCH)
}

pub(super) fn runtime_supplementary_signal_lifecycle_batch_snapshot()
-> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH)
}

fn family_batch_entries(
    family: &str,
) -> std::result::Result<&'static [NodeCompatBatchEntry], String> {
    match family {
        "core-semantics" => Ok(CORE_SEMANTICS_BATCH),
        "process-and-timing" => Ok(PROCESS_AND_TIMING_BATCH),
        "process-and-timing-supplementary" => Ok(PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH),
        "streams-and-local-io" => Ok(STREAMS_AND_LOCAL_IO_BATCH),
        "networking" => Ok(NETWORKING_BATCH),
        "loader-context" => Ok(LOADER_CONTEXT_BATCH),
        "loader-context-supplementary" => Ok(LOADER_CONTEXT_SUPPLEMENTARY_BATCH),
        "loader-context-supplementary-module-bridge" => {
            Ok(LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH)
        }
        "loader-context-supplementary-global-injection" => {
            Ok(LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH)
        }
        "runtime-supplementary" => Ok(RUNTIME_SUPPLEMENTARY_BATCH),
        "runtime-supplementary-signal-lifecycle" => {
            Ok(RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH)
        }
        other => Err(format!("unsupported seeded family catalog `{other}`")),
    }
}

macro_rules! shared_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($fixture_source_path),
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_batch_case_with_extra {
    ($test_relative_path:literal, $fixture_source_path:literal, $extra_files:expr) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($fixture_source_path),
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: $extra_files,
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! split_batch_case {
    ($test_relative_path:literal, $node20_fixture_source_path:literal, $node22_fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($node20_fixture_source_path),
            node22_fixture_source_path: Some($node22_fixture_source_path),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_lane_fixture_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($fixture_source_path),
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: Some($fixture_source_path),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! node20_only_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($fixture_source_path),
            node22_fixture_source_path: None,
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! node22_only_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: None,
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! node22_default_only_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: None,
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: None,
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_official_batch_case {
    ($test_relative_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some(concat!("node20/", $test_relative_path)),
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_official_batch_case_with_extra {
    ($test_relative_path:literal, $extra_files:expr) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some(concat!("node20/", $test_relative_path)),
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: $extra_files,
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_node20_node22_batch_case_with_extra {
    ($test_relative_path:literal, $extra_files:expr) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            shared_extra_files: $extra_files,
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_node20_node22_with_node24_override_case_with_extra {
    ($test_relative_path:literal, $node24_fixture_source_path:literal, $extra_files:expr) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some($node24_fixture_source_path),
            shared_extra_files: $extra_files,
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

const NODE20_ASSERT_FIRST_LINE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/assert-first-line.js",
        fixture_source_path: "node20/test/fixtures/assert-first-line.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/assert-long-line.js",
        fixture_source_path: "node20/test/fixtures/assert-long-line.js",
    },
];

const NODE20_CONSOLE_GROUP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/hijackstdio.js",
        fixture_source_path: "node20/test/common/hijackstdio.js",
    }];

const COMMON_HIJACKSTDIO_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    NODE20_CONSOLE_GROUP_EXTRA_FILES;

const NODE20_COMMON_INDEX_MJS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node20/test/common/index.mjs",
    }];

const NODE22_COMMON_INDEX_MJS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node22/test/common/index.mjs",
    }];

const NODE24_COMMON_INDEX_MJS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node24/test/common/index.mjs",
    }];

const COMMON_INDEX_MJS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "test/common/index.mjs",
    }];

const COMMON_TICK_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[NodeCompatExtraFixtureEntry {
    runtime_path: "test/common/tick.js",
    fixture_source_path: "test/common/tick.js",
}];

const COMMON_COUNTDOWN_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/countdown.js",
        fixture_source_path: "test/common/countdown.js",
    }];

const COMMON_HTTP2_COMPAT_SERVERREQUEST_PIPE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person-large.jpg",
        fixture_source_path: "test/fixtures/person-large.jpg",
    }];

const COMMON_PERSON_JPG_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg",
        fixture_source_path: "test/fixtures/person.jpg",
    }];

const COMMON_CRYPTO_HASH_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/crypto.js",
        fixture_source_path: "test/common/crypto.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/sample.png",
        fixture_source_path: "test/fixtures/sample.png",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/utf8_test_text.txt",
        fixture_source_path: "test/fixtures/utf8_test_text.txt",
    },
];

const COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/crypto.js",
        fixture_source_path: "test/common/crypto.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/aead-vectors.js",
        fixture_source_path: "test/fixtures/aead-vectors.js",
    },
];

const COMMON_TEST_RUNNER_EVENT_METADATA_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/index.js",
        fixture_source_path: "test/fixtures/test-runner/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/test-id-fixture.js",
        fixture_source_path: "test/fixtures/test-runner/test-id-fixture.js",
    },
];

const COMMON_TEST_RUNNER_PLAN_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "test/common/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.mjs",
        fixture_source_path: "test/common/fixtures.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/less.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/less.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/match.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/match.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/more.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/more.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/nested-subtests.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/nested-subtests.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/plan-via-options.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/plan-via-options.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/streaming.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/streaming.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/subtest.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/subtest.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/timeout-basic.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/timeout-basic.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/timeout-expired.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/timeout-expired.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/timeout-wait-false.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/timeout-wait-false.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/timeout-wait-true.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/timeout-wait-true.mjs",
    },
];

const COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/child_process.js",
        fixture_source_path: "test/common/child_process.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "test/common/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/tmpdir.js",
        fixture_source_path: "test/common/tmpdir.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/syntax-error-test.mjs",
        fixture_source_path: "test/fixtures/test-runner/syntax-error-test.mjs",
    },
];

const COMMON_TEST_RUNNER_REPORTERS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/test-error-reporter.js",
        fixture_source_path: "test/common/test-error-reporter.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/tmpdir.js",
        fixture_source_path: "test/common/tmpdir.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/empty.js",
        fixture_source_path: "test/fixtures/empty.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/reporters.js",
        fixture_source_path: "test/fixtures/test-runner/reporters.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/custom.js",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/custom.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/custom.cjs",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/custom.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/custom.mjs",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/custom.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/throwing.js",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/throwing.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/throwing-async.js",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/throwing-async.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/index.test.js",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/index.test.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/error-reporter-fail-fast/a.mjs",
        fixture_source_path: "test/fixtures/test-runner/error-reporter-fail-fast/a.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/error-reporter-fail-fast/b.mjs",
        fixture_source_path: "test/fixtures/test-runner/error-reporter-fail-fast/b.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/node_modules/reporter-cjs/index.js",
        fixture_source_path: "test/fixtures/test-runner/node_modules/reporter-cjs/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/node_modules/reporter-cjs/package.json",
        fixture_source_path: "test/fixtures/test-runner/node_modules/reporter-cjs/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/node_modules/reporter-esm/index.mjs",
        fixture_source_path: "test/fixtures/test-runner/node_modules/reporter-esm/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/node_modules/reporter-esm/package.json",
        fixture_source_path: "test/fixtures/test-runner/node_modules/reporter-esm/package.json",
    },
];

const COMMON_TEST_RUNNER_CLI_OPTIONS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/index.test.js",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/index.test.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/random.test.mjs",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/random.test.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/subdir/subdir_test.js",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/subdir/subdir_test.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/test/random.cjs",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/test/random.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/test/skip_by_name.cjs",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/test/skip_by_name.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/test/suite_and_test.cjs",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/test/suite_and_test.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/node_modules/test-nm.js",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/node_modules/test-nm.js",
    },
];

const COMMON_TEST_RUNNER_CLI_RANDOMIZE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/randomize/internal-order.cjs",
        fixture_source_path: "test/fixtures/test-runner/randomize/internal-order.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/rerun-state.json",
        fixture_source_path: "test/fixtures/test-runner/rerun-state.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/a.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/a.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/b.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/b.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/c.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/c.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/d.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/d.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/e.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/e.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/f.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/f.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/g.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/g.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/h.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/h.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/i.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/i.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/j.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/j.cjs",
    },
];

const COMMON_TEST_RUNNER_CLI_RERUN_FAILURES_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/rerun.js",
        fixture_source_path: "test/fixtures/test-runner/rerun.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/rerun-state.json",
        fixture_source_path: "test/fixtures/test-runner/rerun-state.json",
    },
];

const COMMON_ZLIB_GZIP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg",
        fixture_source_path: "test/fixtures/person.jpg",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg.gz",
        fixture_source_path: "test/fixtures/person.jpg.gz",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/pseudo-multimember-gzip.z",
        fixture_source_path: "test/fixtures/pseudo-multimember-gzip.z",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/pseudo-multimember-gzip.gz",
        fixture_source_path: "test/fixtures/pseudo-multimember-gzip.gz",
    },
];

const COMMON_ZLIB_BROTLI_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/gc.js",
        fixture_source_path: "test/common/gc.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg",
        fixture_source_path: "test/fixtures/person.jpg",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg.br",
        fixture_source_path: "test/fixtures/person.jpg.br",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/pss-vectors.json",
        fixture_source_path: "test/fixtures/pss-vectors.json",
    },
];

const COMMON_GC_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[NodeCompatExtraFixtureEntry {
    runtime_path: "test/common/gc.js",
    fixture_source_path: "test/common/gc.js",
}];

const COMMON_REPL_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/arraystream.js",
        fixture_source_path: "test/common/arraystream.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/repl.js",
        fixture_source_path: "test/common/repl.js",
    },
];

const NODE22_COMMON_UDP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/udp.js",
        fixture_source_path: "node22/test/common/udp.js",
    }];

const COMMON_TLS_KEY_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-key.pem",
        fixture_source_path: "test/fixtures/keys/agent1-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca1-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/dh2048.pem",
        fixture_source_path: "test/fixtures/keys/dh2048.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-key.pem",
        fixture_source_path: "test/fixtures/keys/agent2-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent2-cert.pem",
    },
];

const COMMON_TLS_KEY_COUNTDOWN_GC_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-key.pem",
        fixture_source_path: "test/fixtures/keys/agent1-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca1-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/dh2048.pem",
        fixture_source_path: "test/fixtures/keys/dh2048.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-key.pem",
        fixture_source_path: "test/fixtures/keys/agent2-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent2-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/countdown.js",
        fixture_source_path: "test/common/countdown.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/gc.js",
        fixture_source_path: "test/common/gc.js",
    },
];

const COMMON_TLS_EXTENDED_CERT_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-key.pem",
        fixture_source_path: "test/fixtures/keys/agent1-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-key.pem",
        fixture_source_path: "test/fixtures/keys/agent2-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent2-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent3-key.pem",
        fixture_source_path: "test/fixtures/keys/agent3-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent3-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent3-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca1-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca2-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca2-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_private.pem",
        fixture_source_path: "test/fixtures/keys/rsa_private.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_cert.crt",
        fixture_source_path: "test/fixtures/keys/rsa_cert.crt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ec-key.pem",
        fixture_source_path: "test/fixtures/keys/ec-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ec-cert.pem",
        fixture_source_path: "test/fixtures/keys/ec-cert.pem",
    },
];

const COMMON_TLS_SESSION_CERT_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-key.pem",
        fixture_source_path: "test/fixtures/keys/agent1-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-key.pem",
        fixture_source_path: "test/fixtures/keys/agent2-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent2-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca1-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_private.pem",
        fixture_source_path: "test/fixtures/keys/rsa_private.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_cert.crt",
        fixture_source_path: "test/fixtures/keys/rsa_cert.crt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_cert.pfx",
        fixture_source_path: "test/fixtures/keys/rsa_cert.pfx",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/selfsigned-no-keycertsign/key.pem",
        fixture_source_path: "test/fixtures/keys/selfsigned-no-keycertsign/key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/selfsigned-no-keycertsign/cert.pem",
        fixture_source_path: "test/fixtures/keys/selfsigned-no-keycertsign/cert.pem",
    },
];

const PATH_RESOLVE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[NodeCompatExtraFixtureEntry {
    runtime_path: "test/fixtures/path-resolve.js",
    fixture_source_path: "node20/test/fixtures/path-resolve.js",
}];

const URL_PARSE_DEPRECATION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/node_modules/url-deprecations.js",
        fixture_source_path: "test/fixtures/node_modules/url-deprecations.js",
    }];

const NODE20_UTIL_PARSE_ENV_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node20/test/fixtures/dotenv/valid.env",
    }];

const NODE22_UTIL_PARSE_ENV_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node22/test/fixtures/dotenv/valid.env",
    }];

const NODE24_UTIL_PARSE_ENV_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node24/test/fixtures/dotenv/valid.env",
    }];

const NODE20_PROCESS_LOAD_ENV_FILE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node20/test/fixtures/dotenv/valid.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/.env",
        fixture_source_path: "test/fixtures/dotenv/.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/basic-valid.env",
        fixture_source_path: "test/fixtures/dotenv/basic-valid.env",
    },
];

const NODE22_PROCESS_LOAD_ENV_FILE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node22/test/fixtures/dotenv/valid.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/.env",
        fixture_source_path: "test/fixtures/dotenv/.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/basic-valid.env",
        fixture_source_path: "test/fixtures/dotenv/basic-valid.env",
    },
];

const NODE24_PROCESS_LOAD_ENV_FILE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node24/test/fixtures/dotenv/valid.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/.env",
        fixture_source_path: "test/fixtures/dotenv/.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/basic-valid.env",
        fixture_source_path: "test/fixtures/dotenv/basic-valid.env",
    },
];

const MIME_WHATWG_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/mime-whatwg.js",
        fixture_source_path: "test/fixtures/mime-whatwg.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/mime-whatwg-generated.js",
        fixture_source_path: "test/fixtures/mime-whatwg-generated.js",
    },
];

const STREAM_FLATMAP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[NodeCompatExtraFixtureEntry {
    runtime_path: "test/fixtures/x.txt",
    fixture_source_path: "test/fixtures/x.txt",
}];

const SHARED_FIXTURES_DIR_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/a.js",
        fixture_source_path: "test/fixtures/a.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/baz.js",
        fixture_source_path: "test/fixtures/baz.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/empty.js",
        fixture_source_path: "test/fixtures/empty.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/x.txt",
        fixture_source_path: "test/fixtures/x.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/empty.txt",
        fixture_source_path: "test/fixtures/empty.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/elipses.txt",
        fixture_source_path: "test/fixtures/elipses.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/utf8_test_text.txt",
        fixture_source_path: "test/fixtures/utf8_test_text.txt",
    },
];

const CYCLE_FIXTURES_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/cycles/root.js",
        fixture_source_path: "test/fixtures/cycles/root.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/cycles/folder/foo.js",
        fixture_source_path: "test/fixtures/cycles/folder/foo.js",
    },
];

const MODULE_COMMONJS_FIXTURES_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/experimental.json",
        fixture_source_path: "test/fixtures/experimental.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/utf/新建文件夹/index.js",
        fixture_source_path: "test/fixtures/copy/utf/新建文件夹/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/utf/新建文件夹/experimental.json",
        fixture_source_path: "test/fixtures/copy/utf/新建文件夹/experimental.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/GH-7131/a.js",
        fixture_source_path: "test/fixtures/GH-7131/a.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/GH-7131/b.js",
        fixture_source_path: "test/fixtures/GH-7131/b.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/packages/missing-main/index.js",
        fixture_source_path: "test/fixtures/packages/missing-main/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/packages/missing-main/package.json",
        fixture_source_path: "test/fixtures/packages/missing-main/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/shared-lib-util.js",
        fixture_source_path: "test/common/shared-lib-util.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-loading-error.node",
        fixture_source_path: "test/fixtures/module-loading-error.node",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/not-main-module.js",
        fixture_source_path: "test/fixtures/not-main-module.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/cjs-module-wrap.js",
        fixture_source_path: "test/fixtures/cjs-module-wrap.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/cjs-module-wrapper.js",
        fixture_source_path: "test/fixtures/cjs-module-wrapper.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-wrap-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-wrap-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-require-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-require-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-wrap-call-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-wrap-call-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-node-shape-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-node-shape-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-newline-wrap-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-newline-wrap-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-both/.node_libraries/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-both/.node_libraries/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-both/.node_modules/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-both/.node_modules/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-node_libraries/.node_libraries/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-node_libraries/.node_libraries/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-node_modules/.node_modules/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-node_modules/.node_modules/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/local-pkg/node_modules/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/local-pkg/node_modules/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/local-pkg/test.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/local-pkg/test.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/node_path/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/node_path/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-modules/test-esm-ok.mjs",
        fixture_source_path: "test/fixtures/es-modules/test-esm-ok.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-modules/noext",
        fixture_source_path: "test/fixtures/es-modules/noext",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/index.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/explicit-main/entry.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/explicit-main/entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/explicit-main/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/explicit-main/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/entry.js",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/entry.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/entry.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/entry.js",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/entry.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/entry.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-commonjs/entry.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-commonjs/entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-commonjs/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-commonjs/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/no-main-field/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/no-main-field/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/no-main-field/index.js",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/no-main-field/index.js",
    },
];

const INSPECTOR_FRONT_EDGE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/loop.js",
        fixture_source_path: "test/fixtures/loop.js",
    }];

const PROCESS_FINALIZATION_WATCHPOINT_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node20/test/common/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/child_process.js",
        fixture_source_path: "test/common/child_process.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/before-exit.mjs",
        fixture_source_path: "test/fixtures/process/before-exit.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/close.mjs",
        fixture_source_path: "test/fixtures/process/close.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/different-registry-per-thread.mjs",
        fixture_source_path: "test/fixtures/process/different-registry-per-thread.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/gc-not-close.mjs",
        fixture_source_path: "test/fixtures/process/gc-not-close.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/unregister.mjs",
        fixture_source_path: "test/fixtures/process/unregister.mjs",
    },
];

// Deno's node_compat lane scales by treating the vendored Node corpus as data:
// scan files, then let config decide what runs. Mirror that shape here for the
// focused Nimbus subset so future core-semantics expansion adds manifest rows instead of
// more hand-written Rust test wrappers. Keep both Node20 and Node22 fixture
// roots in one manifest so the default and supported lanes do not drift.
include!("batches/core_semantics.rs");
include!("batches/process_and_streams.rs");
include!("batches/networking.rs");
