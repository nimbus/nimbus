function runtimeBundleExecutionEntrypoints() {
  return `async function executeQueryDefinition(definition, request) {
  const ctx = createRuntimeContext(request);
  const runtimeHandler = await getRuntimeHandler(definition);
  if (runtimeHandler) {
    return await runtimeHandler(ctx, request.args ?? {});
  }
  const plan = resolveArgsTemplate(definition.plan, request.args ?? {});
  return await executeResolvedQueryPlan(ctx, plan);
}

async function executePaginatedQueryDefinition(definition, request) {
  const runtimeHandler = await getRuntimeHandler(definition);
  if (runtimeHandler) {
    const result = await runtimeHandler(createRuntimeContext(request), request.args ?? {});
    if (isRuntimeQueryBuilder(result)) {
      if (typeof request.page_size !== "number") {
        throw new Error("paginated runtime invocation missing page_size");
      }
      return await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_query_paginate", {
        builder_id: result.__builderId,
        page_size: request.page_size,
        cursor: request.cursor ?? null,
        host_call_session_id: request.kind + ":" + request.function_name,
      });
    }
    return result;
  }
  const plan = resolveArgsTemplate(definition.plan, request.args ?? {});
  if (typeof request.page_size !== "number") {
    throw new Error("paginated runtime invocation missing page_size");
  }
  return await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_paginated_query", {
    query: plan,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
    host_call_session_id: request.kind + ":" + request.function_name,
  });
}

async function executeMutationDefinition(definition, request) {
  const ctx = createRuntimeContext(request);
  const runtimeHandler = await getRuntimeHandler(definition);
  if (runtimeHandler) {
    return await runtimeHandler(ctx, request.args ?? {});
  }
  const plan = resolveArgsTemplate(definition.plan, request.args ?? {});
  return await executeResolvedMutationPlan(ctx, plan);
}

async function executeActionDefinition(definition, request) {
  const ctx = createRuntimeContext(request);
  const runtimeHandler = await getRuntimeHandler(definition);
  if (runtimeHandler) {
    return await runtimeHandler(ctx, request.args ?? {});
  }
  const plan = resolveArgsTemplate(definition.plan, request.args ?? {});
  return await executeResolvedActionPlan(ctx, plan, request);
}`;
}

export { runtimeBundleExecutionEntrypoints };
