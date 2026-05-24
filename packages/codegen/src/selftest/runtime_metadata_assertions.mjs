import assert from "node:assert/strict";

function assertDefaultRuntimeMetadata(definition) {
  assert.equal(definition.runtime_environment, "default");
  assert.equal(definition.runtime_engine, "v8");
  assert.equal(definition.runtime_bundle_content_kind, "javascript");
  assert.equal(definition.runtime_javascript_evaluation_format, "es_module");
  assert.equal(definition.runtime_compatibility_target, "web_standard_isolate");
  assert.equal(definition.runtime_package_resolution, "bundled");
  assert.equal(definition.node_version, null);
  assert.equal(definition.node_runtime_target, null);
}

function assertNodeRuntimeMetadata(definition, { nodeVersion, runtimeTarget }) {
  assert.equal(definition.runtime_environment, "node");
  assert.equal(definition.runtime_engine, "v8");
  assert.equal(definition.runtime_bundle_content_kind, "javascript");
  assert.equal(definition.runtime_javascript_evaluation_format, "es_module");
  assert.equal(definition.runtime_compatibility_target, runtimeTarget);
  assert.equal(definition.runtime_package_resolution, "node_external_packages");
  assert.equal(definition.node_version, nodeVersion);
  assert.equal(definition.node_runtime_target, runtimeTarget);
}

function assertRuntimeLanes(manifest, selectedNode) {
  assert.deepEqual(manifest.runtime_lanes, {
    default: {
      runtime_engine: "v8",
      runtime_bundle_content_kind: "javascript",
      runtime_javascript_evaluation_format: "es_module",
      runtime_compatibility_target: "web_standard_isolate",
      runtime_package_resolution: "bundled",
    },
    node20: {
      runtime_engine: "v8",
      runtime_bundle_content_kind: "javascript",
      runtime_javascript_evaluation_format: "es_module",
      runtime_compatibility_target: "node20",
      runtime_package_resolution: "node_external_packages",
    },
    node22: {
      runtime_engine: "v8",
      runtime_bundle_content_kind: "javascript",
      runtime_javascript_evaluation_format: "es_module",
      runtime_compatibility_target: "node22",
      runtime_package_resolution: "node_external_packages",
    },
    node24: {
      runtime_engine: "v8",
      runtime_bundle_content_kind: "javascript",
      runtime_javascript_evaluation_format: "es_module",
      runtime_compatibility_target: "node24",
      runtime_package_resolution: "node_external_packages",
    },
    bunJsc: {
      runtime_engine: "bun_jsc",
      runtime_bundle_content_kind: "javascript",
      runtime_javascript_evaluation_format: "program_wrapper",
      runtime_compatibility_target: "bun_jsc",
      runtime_package_resolution: "bun_self_contained",
    },
    selectedNode,
  });
}

export {
  assertDefaultRuntimeMetadata,
  assertNodeRuntimeMetadata,
  assertRuntimeLanes,
};
