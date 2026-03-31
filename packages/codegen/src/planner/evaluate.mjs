import { OPERATION_MARKER, QUERY_STATE_MARKER } from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

import { createContextProxy } from "./context_api.mjs";
import { createArgsProxy } from "./args_proxy.mjs";
import { createHttpCompileBindings } from "./http_compile_bindings.mjs";
import { finalizeQueryState } from "./operations.mjs";
import { createRequestProxy } from "./request_proxy.mjs";
import {
  isOperationMarker,
  isQueryStateMarker,
  sanitizeHttpActionPlan,
  sanitizeTemplate,
} from "./templates.mjs";

async function evaluateResolverPlan(
  resolverText,
  filePath,
  schema = { tables: {} },
  options = {
    withContext: false,
    withRequest: false,
    kind: "query",
    compileBindings: {},
    argsSchema: {},
  },
) {
  if (!resolverText.includes("=>")) {
    throw unsupportedError(filePath, "non-arrow resolver");
  }

  const resolver = compileResolver(
    resolverText,
    filePath,
    {
      ...(options.compileBindings ?? {}),
      ...(options.kind === "http_action" ? createHttpCompileBindings(filePath) : {}),
    },
  );
  const operationLog = [];
  const ctxProxy = createContextProxy(
    filePath,
    schema,
    options.kind,
    operationLog,
    options.argsSchema ?? {},
  );
  const argsProxy = createArgsProxy();
  const requestProxy = options.withRequest ? createRequestProxy(filePath) : undefined;

  let resolved;
  try {
    resolved = options.withContext
      ? options.withRequest
        ? await resolver(ctxProxy, requestProxy)
        : await resolver(ctxProxy, argsProxy)
      : await resolver(argsProxy);
  } catch (error) {
    if (
      error instanceof Error &&
      typeof error.message === "string" &&
      error.message.includes("requires Phase 4C runtime execution support")
    ) {
      throw error;
    }
    throw unsupportedError(filePath, `runtime-only resolver logic (${error.message})`);
  }
  if (options.kind === "http_action") {
    return sanitizeHttpActionPlan(resolved, operationLog, filePath);
  }
  if (operationLog.length > 0) {
    if (operationLog.length !== 1) {
      throw unsupportedError(
        filePath,
        "handlers may compile at most one ctx.db/ctx.scheduler side effect in 4B",
      );
    }
    if (!isOperationMarker(resolved)) {
      throw unsupportedError(
        filePath,
        "handlers using ctx.db/ctx.scheduler must return the compiled operation result in 4B",
      );
    }
    return sanitizeTemplate(operationLog[resolved[OPERATION_MARKER]], filePath);
  }
  if (options.kind === "paginated_query" && isQueryStateMarker(resolved)) {
    return sanitizeTemplate(
      finalizeQueryState(resolved[QUERY_STATE_MARKER], null),
      filePath,
    );
  }
  return sanitizeTemplate(resolved, filePath);
}

function compileResolver(resolverText, filePath, compileBindings = {}) {
  try {
    const bindingNames = Object.keys(compileBindings);
    return new Function(...bindingNames, `return (${resolverText});`)(
      ...bindingNames.map((name) => compileBindings[name]),
    );
  } catch (error) {
    throw unsupportedError(filePath, `resolver parsing (${error.message})`);
  }
}

export { evaluateResolverPlan };
