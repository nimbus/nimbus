import { unsupportedError } from "../errors.mjs";
import { evaluateResolverPlan } from "../planner.mjs";
import { findCallOpenParen, splitTopLevel } from "../syntax.mjs";

import { parseServerDefinition } from "./server_definition.mjs";

async function parseHttpActionCall(callExpression, filePath, schema, compileBindings) {
  const openParen = findCallOpenParen(callExpression, "httpAction".length, filePath);
  const args = splitTopLevel(callExpression.slice(openParen + 1, -1), ",", filePath);
  if (args.length !== 1) {
    throw unsupportedError(filePath, "httpAction(...) arity");
  }
  return parseHttpActionDefinition(
    args[0].trim(),
    filePath,
    schema,
    compileBindings,
  );
}

async function parseHttpActionDefinition(
  definitionText,
  filePath,
  schema,
  compileBindings,
) {
  const handlerExpression = definitionText.startsWith("{")
    ? parseHttpActionObjectDefinition(definitionText, filePath)
    : definitionText;
  return evaluateResolverPlan(handlerExpression, filePath, schema, {
    withContext: true,
    withRequest: true,
    kind: "http_action",
    compileBindings,
    argsSchema: {},
  });
}

function parseHttpActionObjectDefinition(definitionText, filePath) {
  const definition = parseServerDefinition(definitionText, filePath);
  if (Object.keys(definition.argsSchema).length > 0 || definition.returnsSchema !== undefined) {
    throw unsupportedError(filePath, "httpAction does not support args or returns metadata");
  }
  return definition.handlerExpression;
}

export { parseHttpActionCall, parseHttpActionDefinition };
