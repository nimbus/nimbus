function runtimeBundleDispatchInvocation() {
  return `function invokeNamedDefinition(name, expectedKind, args, options = {}) {
  const definition = functionsByName.get(name);
  if (!definition) {
    throw new Error(\`nimbus function not found: \${name}\`);
  }
  if (definition.kind !== expectedKind) {
    throw new Error(
      \`nimbus function kind mismatch for \${name}: expected \${expectedKind}, got \${definition.kind}\`,
    );
  }

  const request = {
    kind: expectedKind,
    function_name: name,
    args,
    page_size: options.pageSize,
    cursor: options.cursor ?? null,
    hostCallSessionId: options.hostCallSessionId,
  };

  switch (expectedKind) {
    case "query":
      return executeQueryDefinition(definition, request);
    case "paginated_query":
      return executePaginatedQueryDefinition(definition, request);
    case "mutation":
      return executeMutationDefinition(definition, request);
    case "action":
      return executeActionDefinition(definition, request);
    default:
      throw new Error(\`unsupported nimbus function kind: \${expectedKind}\`);
  }
}

async function invokeNamedDefinitionLocally(request) {
  const definition = functionsByName.get(request.function_name);
  if (!definition) {
    throw new Error("nimbus function not found: " + request.function_name);
  }
  // request.visibility is only supplied by same-isolate nested ctx.run*
  // dispatch, where it carries the generated reference tree the caller used
  // (api.* vs internal.*) and the host never sees the call — so the
  // reference-selection check must run here. A host-constructed invocation
  // (client traffic, the scheduler, or a cross-lane nested ctx.run*
  // re-entering this bundle through host dispatch) omits it: the host has
  // already resolved and enforced visibility against its registry, and an
  // internal function reached that way is a trusted server-side call, not a
  // public one.
  if (
    typeof request.visibility === "string"
    && definition.visibility !== request.visibility
  ) {
    throw new Error(
      "nimbus function "
        + request.function_name
        + " is "
        + definition.visibility
        + ", not "
        + request.visibility,
    );
  }
  if (definition.kind !== request.kind) {
    throw new Error(
      "nimbus function kind mismatch for "
        + request.function_name
        + ": expected "
        + request.kind
        + ", got "
        + definition.kind,
    );
  }
  return invokeNamedDefinition(request.function_name, request.kind, request.args ?? {}, {
    pageSize: request.page_size,
    cursor: request.cursor ?? null,
    hostCallSessionId: request.hostCallSessionId,
  });
}`;
}

export { runtimeBundleDispatchInvocation };
