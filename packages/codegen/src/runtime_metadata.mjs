const RUNTIME_ENGINE = "v8";
const RUNTIME_BUNDLE_CONTENT_KIND = "javascript";
const JAVASCRIPT_EVALUATION_FORMAT = "es_module";
const DEFAULT_COMPATIBILITY_TARGET = "web_standard_isolate";
const NODE_PACKAGE_RESOLUTION = "node_external_packages";
const BUNDLED_PACKAGE_RESOLUTION = "bundled";

function runtimeMetadataForFunction({ runtimeEnvironment, projectConfig }) {
  const runtimeCompatibilityTarget = runtimeEnvironment === "node"
    ? projectConfig.node.runtimeTarget
    : DEFAULT_COMPATIBILITY_TARGET;
  return {
    runtime_engine: RUNTIME_ENGINE,
    runtime_bundle_content_kind: RUNTIME_BUNDLE_CONTENT_KIND,
    runtime_javascript_evaluation_format: JAVASCRIPT_EVALUATION_FORMAT,
    runtime_compatibility_target: runtimeCompatibilityTarget,
    runtime_package_resolution: runtimeEnvironment === "node"
      ? NODE_PACKAGE_RESOLUTION
      : BUNDLED_PACKAGE_RESOLUTION,
  };
}

function runtimeLaneMetadata(projectConfig) {
  const nodeLane = (runtimeTarget) => ({
    runtime_engine: RUNTIME_ENGINE,
    runtime_bundle_content_kind: RUNTIME_BUNDLE_CONTENT_KIND,
    runtime_javascript_evaluation_format: JAVASCRIPT_EVALUATION_FORMAT,
    runtime_compatibility_target: runtimeTarget,
    runtime_package_resolution: NODE_PACKAGE_RESOLUTION,
  });
  return {
    default: {
      runtime_engine: RUNTIME_ENGINE,
      runtime_bundle_content_kind: RUNTIME_BUNDLE_CONTENT_KIND,
      runtime_javascript_evaluation_format: JAVASCRIPT_EVALUATION_FORMAT,
      runtime_compatibility_target: DEFAULT_COMPATIBILITY_TARGET,
      runtime_package_resolution: BUNDLED_PACKAGE_RESOLUTION,
    },
    node20: nodeLane("node20"),
    node22: nodeLane("node22"),
    node24: nodeLane("node24"),
    selectedNode: projectConfig.node.runtimeTarget,
  };
}

export {
  BUNDLED_PACKAGE_RESOLUTION,
  DEFAULT_COMPATIBILITY_TARGET,
  JAVASCRIPT_EVALUATION_FORMAT,
  NODE_PACKAGE_RESOLUTION,
  RUNTIME_BUNDLE_CONTENT_KIND,
  RUNTIME_ENGINE,
  runtimeLaneMetadata,
  runtimeMetadataForFunction,
};
