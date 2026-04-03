import { SUPPORTED_HELPERS } from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";
import { extractCallExpression } from "../syntax.mjs";

import { parseDefineCall, parseServerCall } from "./function_parsers.mjs";
import { parseHttpActionCall } from "./http_action_definitions.mjs";

async function extractFunctionDefinitions(
  source,
  filePath,
  moduleName,
  schema,
  compileBindings,
  runtimeBindings,
) {
  const functions = [];
  const assignmentPattern =
    /export\s+const\s+([A-Za-z_$][\w$]*)\s*=\s*([A-Za-z_$][\w$]*)\b/g;

  for (const match of source.matchAll(assignmentPattern)) {
    const exportName = match[1];
    const helperName = match[2];
    const helper = SUPPORTED_HELPERS.get(helperName);
    if (!helper) {
      throw unsupportedError(filePath, `unsupported "${exportName}" export`);
    }

    const callStart = match.index + match[0].lastIndexOf(helperName);
    const callExpression = extractCallExpression(source, callStart, filePath);
    const parsed =
      helper.mode === "define"
        ? await parseDefineCall(callExpression, helperName, filePath)
        : await parseServerCall(
            callExpression,
            helper,
            helperName,
            filePath,
            schema,
            compileBindings,
          );

    functions.push({
      exportName,
      name: helper.mode === "define" ? parsed.name : `${moduleName}:${exportName}`,
      kind: helper.kind,
      visibility: helper.visibility,
      plan: parsed.plan,
      runtimeHandler: parsed.runtimeHandler,
      runtimeBindings,
      argsSchema: parsed.argsSchema ?? {},
      returnsSchema: parsed.returnsSchema,
    });
  }

  return functions;
}

export { extractFunctionDefinitions, parseHttpActionCall };
