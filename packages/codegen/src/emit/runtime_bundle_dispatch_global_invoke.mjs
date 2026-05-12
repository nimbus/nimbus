function runtimeBundleDispatchGlobalInvoke() {
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

    throw new Error(\`convex function or route not found: \${request.function_name}\`);
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

export {};`;
}

export { runtimeBundleDispatchGlobalInvoke };
