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

// Per-function runtime lane metadata for the nested ctx.run* dispatcher: the
// host-owned context contract compares the callee's runtime_environment
// against the lane this isolate executes and routes cross-lane calls through
// host dispatch (the engine path) instead of same-isolate local dispatch.
globalThis.__nimbusLocalFunctionRuntimeEnvironment = function (name) {
  const definition = functionsByName.get(name);
  return definition && typeof definition.runtime_environment === "string"
    ? definition.runtime_environment
    : null;
};${moduleSentinel}`;
}

export { runtimeBundleDispatchGlobalInvoke };
