import { runtimeBundleDispatch } from "./runtime_bundle_dispatch.mjs";
import { runtimeBundleExecution } from "./runtime_bundle_execution.mjs";
import { runtimeBundlePreamble } from "./runtime_bundle_preamble.mjs";

function buildRuntimeBundleSource(manifestJson, importPreamble = "") {
  return [
    importPreamble,
    runtimeBundlePreamble(manifestJson),
    runtimeBundleExecution(),
    runtimeBundleDispatch(),
  ].filter(Boolean).join("\n\n");
}

export { buildRuntimeBundleSource };
