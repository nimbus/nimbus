import { runtimeBundleDispatch } from "./runtime_bundle_dispatch.mjs";
import { runtimeBundleExecution } from "./runtime_bundle_execution.mjs";
import { runtimeBundlePreamble } from "./runtime_bundle_preamble.mjs";

function buildRuntimeBundleSource(manifestJson, options = {}) {
  return [
    runtimeBundlePreamble(manifestJson),
    runtimeBundleExecution(),
    runtimeBundleDispatch(options),
  ].filter(Boolean).join("\n\n");
}

export { buildRuntimeBundleSource };
