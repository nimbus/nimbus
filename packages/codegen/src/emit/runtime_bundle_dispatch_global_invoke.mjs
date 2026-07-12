function runtimeBundleDispatchGlobalInvoke({ module = true } = {}) {
  const moduleSentinel = module ? "\n\nexport {};" : "";
  return `globalThis.__nimbusInvoke = async function (request) {
  try {
    const definition = functionsByName.get(request.function_name);
    if (definition) {
      return { status: "ok", value: await invokeNamedDefinitionLocally(request) };
    }

    const route = request.kind === "action"
      ? routesByName.get(request.function_name)
      : undefined;
    if (route) {
      return await globalThis.__nimbusAsyncHostValue("op_nimbus_http_route", {
        request,
        route,
      });
    }

    throw new Error(\`nimbus function or route not found: \${request.function_name}\`);
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return {
        status: "error",
        error: error.nimbusHostError,
      };
    }
    throw error;
  }
};

globalThis.__nimbusInvokeNamedLocal = invokeNamedDefinitionLocally;

// The nested ctx.run* dispatcher resolves each callee's runtime lane HOST-side
// (op_nimbus_ctx_resolve_callee_lane against the host registry), so the bundle
// deliberately publishes NO per-function lane lookup or registrar. There is no
// guest-reachable JavaScript state a handler body or an eagerly-imported
// dependency could tamper with to force a cross-lane callee onto same-isolate
// local dispatch.${moduleSentinel}`;
}

export { runtimeBundleDispatchGlobalInvoke };
