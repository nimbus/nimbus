use super::{NodeCompatBatchEntry, NodeCompatExtraFixtureEntry};

const SUPPLEMENTARY_MODULE_RESOLUTION_BRIDGE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "node_modules/bridge-target/package.json",
        fixture_source_path: "supplementary/fixtures/module-resolution-bridge/node_modules/bridge-target/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "node_modules/bridge-target/esm-entry.mjs",
        fixture_source_path: "supplementary/fixtures/module-resolution-bridge/node_modules/bridge-target/esm-entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "node_modules/bridge-target/esm-feature.mjs",
        fixture_source_path: "supplementary/fixtures/module-resolution-bridge/node_modules/bridge-target/esm-feature.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "node_modules/bridge-target/cjs-entry.cjs",
        fixture_source_path: "supplementary/fixtures/module-resolution-bridge/node_modules/bridge-target/cjs-entry.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "node_modules/bridge-target/cjs-feature.cjs",
        fixture_source_path: "supplementary/fixtures/module-resolution-bridge/node_modules/bridge-target/cjs-feature.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "supplementary/fixtures/module-resolution-bridge/commonjs-only.cjs",
        fixture_source_path: "supplementary/fixtures/module-resolution-bridge/commonjs-only.cjs",
    },
];

const SUPPLEMENTARY_GLOBAL_INJECTION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "supplementary/fixtures/global-injection-fidelity/esm-shape.mjs",
        fixture_source_path: "supplementary/fixtures/global-injection-fidelity/esm-shape.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "supplementary/fixtures/global-injection-fidelity/cjs-shape.cjs",
        fixture_source_path: "supplementary/fixtures/global-injection-fidelity/cjs-shape.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "supplementary/fixtures/global-injection-fidelity/cjs-required-value.cjs",
        fixture_source_path: "supplementary/fixtures/global-injection-fidelity/cjs-required-value.cjs",
    },
];

const SUPPLEMENTARY_FRAMEWORK_LOADER_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "supplementary/fixtures/framework-loader-patterns/message.fixture",
        fixture_source_path: "supplementary/fixtures/framework-loader-patterns/message.fixture",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "supplementary/fixtures/framework-loader-patterns/package-entry/package.json",
        fixture_source_path: "supplementary/fixtures/framework-loader-patterns/package-entry/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "supplementary/fixtures/framework-loader-patterns/package-entry/main.cjs",
        fixture_source_path: "supplementary/fixtures/framework-loader-patterns/package-entry/main.cjs",
    },
];

pub(super) const LOADER_CONTEXT_SUPPLEMENTARY_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "supplementary/builtin-completeness.mjs",
        node20_fixture_source_path: Some("supplementary/builtin-completeness.mjs"),
        node22_fixture_source_path: Some("supplementary/builtin-completeness.mjs"),
        node24_fixture_source_path: Some("supplementary/builtin-completeness.mjs"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

pub(super) const LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "supplementary/module-resolution-bridge.mjs",
        node20_fixture_source_path: Some("supplementary/module-resolution-bridge.mjs"),
        node22_fixture_source_path: Some("supplementary/module-resolution-bridge.mjs"),
        node24_fixture_source_path: Some("supplementary/module-resolution-bridge.mjs"),
        shared_extra_files: SUPPLEMENTARY_MODULE_RESOLUTION_BRIDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

pub(super) const LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "supplementary/global-injection-fidelity.mjs",
        node20_fixture_source_path: Some("supplementary/global-injection-fidelity.mjs"),
        node22_fixture_source_path: Some("supplementary/global-injection-fidelity.mjs"),
        node24_fixture_source_path: Some("supplementary/global-injection-fidelity.mjs"),
        shared_extra_files: SUPPLEMENTARY_GLOBAL_INJECTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

pub(super) const PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "supplementary/process-release-shape.js",
        node20_fixture_source_path: Some("supplementary/process-release-shape.node20.js"),
        node22_fixture_source_path: Some("supplementary/process-release-shape.node22.js"),
        node24_fixture_source_path: Some("supplementary/process-release-shape.node24.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

pub(super) const RUNTIME_SUPPLEMENTARY_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "supplementary/resource-safety.mjs",
        node20_fixture_source_path: Some("supplementary/resource-safety.mjs"),
        node22_fixture_source_path: Some("supplementary/resource-safety.mjs"),
        node24_fixture_source_path: Some("supplementary/resource-safety.mjs"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "supplementary/framework-loader-patterns.mjs",
        node20_fixture_source_path: Some("supplementary/framework-loader-patterns.mjs"),
        node22_fixture_source_path: Some("supplementary/framework-loader-patterns.mjs"),
        node24_fixture_source_path: Some("supplementary/framework-loader-patterns.mjs"),
        shared_extra_files: SUPPLEMENTARY_FRAMEWORK_LOADER_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

pub(super) const RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "supplementary/signal-listener-lifecycle.mjs",
        node20_fixture_source_path: Some("supplementary/signal-listener-lifecycle.mjs"),
        node22_fixture_source_path: Some("supplementary/signal-listener-lifecycle.mjs"),
        node24_fixture_source_path: Some("supplementary/signal-listener-lifecycle.mjs"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];
