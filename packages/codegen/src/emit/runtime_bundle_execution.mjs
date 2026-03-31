import { runtimeBundleExecutionEntrypoints } from "./runtime_bundle_execution_entrypoints.mjs";
import { runtimeBundleMutationHelpers } from "./runtime_bundle_mutation_helpers.mjs";
import { runtimeBundleQueryHelpers } from "./runtime_bundle_query_helpers.mjs";

function runtimeBundleExecution() {
  return [
    runtimeBundleExecutionEntrypoints(),
    runtimeBundleQueryHelpers(),
    runtimeBundleMutationHelpers(),
  ].join("\n\n");
}

export { runtimeBundleExecution };
