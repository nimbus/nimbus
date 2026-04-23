import { SUPPORTED_HELPERS } from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

import { parseDefineCall, parseServerCall } from "./function_parsers.mjs";
import { parseHttpActionCall } from "./http_action_definitions.mjs";
import { extractExportedConstAssignments } from "./source_exports.mjs";

async function extractFunctionDefinitions(
  source,
  filePath,
  moduleName,
  schema,
  compileBindings,
  runtimeBindings,
) {
  const functions = [];

  for (const assignment of extractExportedConstAssignments(source, filePath)) {
    const { exportName, helperName, callExpression } = assignment;
    const helper = SUPPORTED_HELPERS.get(helperName);
    if (!exportName || !helper || !callExpression) {
      throw unsupportedError(filePath, `unsupported "${exportName}" export`);
    }

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
