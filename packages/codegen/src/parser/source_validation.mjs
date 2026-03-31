import { SUPPORTED_HELPERS } from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

function ensureSupportedSource(filePath, source) {
  const unsupportedExports = [
    /export\s+default\b/,
    /export\s+function\b/,
    /export\s+class\b/,
    /export\s*\{/,
  ];
  for (const pattern of unsupportedExports) {
    if (pattern.test(source)) {
      throw unsupportedError(filePath, "unsupported export shape");
    }
  }

  const exportAssignments = [
    ...source.matchAll(/export\s+const\s+([A-Za-z_$][\w$]*)\s*=\s*/g),
  ].map((match) => match[1]);
  const supportedAssignments = new Set(
    [...source.matchAll(
      /export\s+const\s+([A-Za-z_$][\w$]*)\s*=\s*([A-Za-z_$][\w$]*)\b/g,
    )]
      .filter((match) => SUPPORTED_HELPERS.has(match[2]))
      .map((match) => match[1]),
  );

  for (const exportName of exportAssignments) {
    if (!supportedAssignments.has(exportName)) {
      throw unsupportedError(filePath, `unsupported "${exportName}" export`);
    }
  }
}

export { ensureSupportedSource };
