import { unsupportedError } from "../errors.mjs";
import { evaluateResolverPlan } from "../planner.mjs";
import {
  findCallOpenParen,
  parseStringLiteral,
  splitTopLevel,
} from "../syntax.mjs";

import { parseHttpActionDefinition } from "./http_action_definitions.mjs";
import { parseServerDefinition } from "./server_definition.mjs";

async function parseDefineCall(callExpression, helperName, filePath) {
  const openParen = findCallOpenParen(callExpression, helperName.length, filePath);
  const args = splitTopLevel(callExpression.slice(openParen + 1, -1), ",", filePath);
  if (args.length !== 2) {
    throw unsupportedError(filePath, "define call arity");
  }

  return {
    name: parseStringLiteral(args[0].trim(), filePath),
    plan: await evaluateResolverPlan(args[1].trim(), filePath),
    argsSchema: {},
    returnsSchema: undefined,
  };
}

async function parseServerCall(
  callExpression,
  helper,
  helperName,
  filePath,
  schema,
  compileBindings,
) {
  const openParen = findCallOpenParen(callExpression, helperName.length, filePath);
  const args = splitTopLevel(callExpression.slice(openParen + 1, -1), ",", filePath);
  if (args.length !== 1) {
    throw unsupportedError(filePath, "server wrapper arity");
  }

  if (helperName === "httpAction") {
    return {
      plan: await parseHttpActionDefinition(
        args[0].trim(),
        filePath,
        schema,
        compileBindings,
      ),
      argsSchema: {},
      returnsSchema: undefined,
    };
  }

  const definition = parseServerDefinition(args[0].trim(), filePath, compileBindings);
  let plan;
  let runtimeHandler;
  try {
    plan = await evaluateResolverPlan(definition.handlerExpression, filePath, schema, {
      withContext: true,
      kind: helper.kind,
      compileBindings,
      argsSchema: definition.argsSchema,
    });
  } catch (error) {
    if (canFallbackToRuntimeHandler(helper.kind, error)) {
      plan = null;
      runtimeHandler = definition.handlerExpression;
    } else {
      throw error;
    }
  }
  return {
    plan,
    runtimeHandler,
    argsSchema: definition.argsSchema,
    returnsSchema: definition.returnsSchema,
  };
}

function canFallbackToRuntimeHandler(kind, error) {
  if (
    kind !== "query"
    && kind !== "paginated_query"
    && kind !== "mutation"
    && kind !== "action"
  ) {
    return false;
  }
  if (!(error instanceof Error) || typeof error.message !== "string") {
    return false;
  }
  return [
    "runtime-only resolver logic",
    "handlers may compile at most one ctx.db/ctx.scheduler side effect in 4B",
    "handlers using ctx.db/ctx.scheduler must return the compiled operation result in 4B",
  ].some((fragment) => error.message.includes(fragment));
}

export { parseDefineCall, parseServerCall };
