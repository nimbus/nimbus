import { SUPPORTED_HELPERS } from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

import {
  extractExportedConstAssignments,
  hasUnsupportedExportShape,
} from "./source_exports.mjs";

function ensureSupportedSource(filePath, source) {
  if (hasUnsupportedExportShape(source, filePath)) {
    throw unsupportedError(filePath, "unsupported export shape");
  }

  for (const { exportName, helperName } of extractExportedConstAssignments(
    source,
    filePath,
  )) {
    if (!exportName || !SUPPORTED_HELPERS.has(helperName)) {
      throw unsupportedError(filePath, `unsupported "${exportName}" export`);
    }
  }
}

export { ensureSupportedSource };
