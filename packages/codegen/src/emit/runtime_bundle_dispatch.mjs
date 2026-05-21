import { runtimeBundleActionHelpers } from "./runtime_bundle_action_helpers.mjs";
import { runtimeBundleDispatchGlobalInvoke } from "./runtime_bundle_dispatch_global_invoke.mjs";
import { runtimeBundleDispatchInvocation } from "./runtime_bundle_dispatch_invocation.mjs";

function runtimeBundleDispatch(options = {}) {
  return [
    runtimeBundleDispatchInvocation(),
    runtimeBundleActionHelpers(),
    runtimeBundleDispatchGlobalInvoke(options),
  ].join("\n\n");
}

export { runtimeBundleDispatch };
