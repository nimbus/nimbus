import { unsupportedError } from "../errors.mjs";

import { parseHttpActionCall } from "./function_definitions.mjs";

async function resolveHttpRouteHandler(
  handlerExpression,
  filePath,
  schema,
  compileBindings,
  importedHttpActions,
  inlineIndex,
) {
  const localNameMatch = /^([A-Za-z_$][\w$]*)$/.exec(handlerExpression);
  if (localNameMatch) {
    const functionInfo = importedHttpActions.get(localNameMatch[1]);
    if (!functionInfo) {
      throw unsupportedError(
        filePath,
        "http route handlers must be inline httpAction(...) calls or imported httpAction exports",
      );
    }
    return {
      name: functionInfo.name,
      plan: functionInfo.plan,
    };
  }

  if (/^httpAction\b/.test(handlerExpression)) {
    return {
      name: `http:inline:${inlineIndex}`,
      plan: await parseHttpActionCall(
        handlerExpression,
        filePath,
        schema,
        compileBindings,
      ),
    };
  }

  throw unsupportedError(
    filePath,
    "http route handlers must be inline httpAction(...) calls or imported httpAction exports",
  );
}

export { resolveHttpRouteHandler };
