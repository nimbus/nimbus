function runtimeBundleDispatchInvocation() {
  return `function invokeNamedDefinition(name, expectedKind, args, options = {}) {
  const definition = functionsByName.get(name);
  if (!definition) {
    throw new Error(\`convex function not found: \${name}\`);
  }
  if (definition.kind !== expectedKind) {
    throw new Error(
      \`convex function kind mismatch for \${name}: expected \${expectedKind}, got \${definition.kind}\`,
    );
  }

  const request = {
    kind: expectedKind,
    function_name: name,
    args,
    page_size: options.pageSize,
    cursor: options.cursor ?? null,
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
      throw new Error(\`unsupported convex function kind: \${expectedKind}\`);
  }
}

async function invokeNamedDefinitionLocally(request) {
  const definition = functionsByName.get(request.function_name);
  if (!definition) {
    throw new Error("convex function not found: " + request.function_name);
  }
  const requestVisibility =
    typeof request.visibility === "string" ? request.visibility : "public";
  if (definition.visibility !== requestVisibility) {
    throw new Error(
      "convex function "
        + request.function_name
        + " is "
        + definition.visibility
        + ", not "
        + requestVisibility,
    );
  }
  if (definition.kind !== request.kind) {
    throw new Error(
      "convex function kind mismatch for "
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
  });
}`;
}

export { runtimeBundleDispatchInvocation };
