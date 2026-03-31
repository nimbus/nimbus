import { runtimeBundleActionHelpers } from "./runtime_bundle_action_helpers.mjs";
import { runtimeBundleDispatchGlobalInvoke } from "./runtime_bundle_dispatch_global_invoke.mjs";
import { runtimeBundleDispatchInvocation } from "./runtime_bundle_dispatch_invocation.mjs";

function runtimeBundleDispatch() {
  return [
    runtimeBundleDispatchInvocation(),
    runtimeBundleActionHelpers(),
    runtimeBundleDispatchGlobalInvoke(),
  ].join("\n\n");
}

export { runtimeBundleDispatch };
