function runtimeBundleDispatchGlobalInvoke({ module = true } = {}) {
  const moduleSentinel = module ? "\n\nexport {};" : "";
  return `// HG0 (Band B-FIX, CAPTURE-ORDERING): installed via Object.defineProperty
// with configurable:false, writable:false rather than a plain assignment, and
// as the FIRST statement this dispatch segment of the bundle runs — before any
// guest handler body has a chance to execute (handler bodies initialize lazily
// on first invocation; see runtime_bundle_preamble.mjs).
// The host still captures this entrypoint off-graph (captured_dispatch.rs)
// after evaluation and event-loop drain, but a guest reassignment attempt —
// whether a direct top-level write or one queued via queueMicrotask — now
// throws instead of silently installing an impostor for capture to observe.
Object.defineProperty(globalThis, "__nimbusInvoke", {
  value: async function (request) {
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
    // The handler's own throw (marked by nimbusRemapHandlerError) is the
    // developer's error, not a runtime fault: it answers as a
    // \`function_thrown\` envelope so the message and the stack reach the
    // caller intact. Everything else (a missing function, a visibility gate)
    // still escapes into the runtime as a service fault.
    if (error && typeof error === "object" && error.nimbusFunctionThrown === true) {
      const stack = typeof error.nimbusOriginalStack === "string"
        ? error.nimbusOriginalStack
        : (typeof error.stack === "string" ? error.stack : null);
      return {
        status: "error",
        error: {
          kind: "function_thrown",
          function_path: request.function_name,
          message: String(error.message == null ? error : error.message),
          stack,
        },
      };
    }
    throw error;
  }
  },
  configurable: false,
  enumerable: false,
  writable: false,
});

// HG2: invokeNamedDefinitionLocally is no longer bridged onto globalThis.
// __nimbusCreateContext receives it directly as a call argument (see
// runtime_bundle_preamble.mjs createRuntimeContext) so a guest handler body
// has no guest-reachable name to reassign and redirect a later same-tenant
// invocation's nested ctx.run* dispatch on a warm isolate.

// The nested ctx.run* dispatcher resolves each callee's runtime lane HOST-side
// (op_nimbus_ctx_resolve_callee_lane against the host registry), so the bundle
// deliberately publishes NO per-function lane lookup or registrar. There is no
// guest-reachable JavaScript state a handler body or an eagerly-imported
// dependency could tamper with to force a cross-lane callee onto same-isolate
// local dispatch.${moduleSentinel}`;
}

export { runtimeBundleDispatchGlobalInvoke };
