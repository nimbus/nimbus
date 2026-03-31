import path from "node:path";

function createLocalHttpActionImportMap(source, convexDir, filePath, functionIndex) {
  const mapping = new Map();
  const importPattern = /import\s*\{([\s\S]*?)\}\s*from\s*["']([^"']+)["'];?/g;
  for (const match of source.matchAll(importPattern)) {
    const specifiers = match[1]
      .split(",")
      .map((specifier) => specifier.trim())
      .filter(Boolean);
    const sourcePath = match[2];
    if (!sourcePath.startsWith(".")) {
      continue;
    }
    if (sourcePath.startsWith("./_generated/")) {
      continue;
    }

    const moduleName = localModuleNameForImport(convexDir, filePath, sourcePath);
    for (const specifier of specifiers) {
      const [importedName, localName = importedName] = specifier
        .split(/\s+as\s+/)
        .map((part) => part.trim());
      const functionName = `${moduleName}:${importedName}`;
      const functionInfo = functionIndex.get(functionName);
      if (functionInfo?.kind === "http_action") {
        mapping.set(localName, functionInfo);
      }
    }
  }
  return mapping;
}

function localModuleNameForImport(convexDir, filePath, sourcePath) {
  const resolved = path.resolve(path.dirname(filePath), sourcePath);
  const relative = path.relative(convexDir, resolved).replaceAll(path.sep, "/");
  return relative.replace(/\.(tsx|ts)$/, "").replaceAll("/", ".");
}

export { createLocalHttpActionImportMap };
