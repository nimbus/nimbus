import { runtimeBundleDispatch } from "./runtime_bundle_dispatch.mjs";
import { runtimeBundleExecution } from "./runtime_bundle_execution.mjs";
import { runtimeBundlePreamble } from "./runtime_bundle_preamble.mjs";

function buildRuntimeBundleSource(manifestJson) {
  return [
    runtimeBundlePreamble(manifestJson),
    runtimeBundleExecution(),
    runtimeBundleDispatch(),
  ].join("\n\n");
}

export { buildRuntimeBundleSource };
