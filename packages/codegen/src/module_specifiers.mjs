import { builtinModules } from "node:module";

const BUILTIN_MODULES = new Set(
  builtinModules.flatMap((specifier) => {
    const bare = specifier.startsWith("node:") ? specifier.slice("node:".length) : specifier;
    return [bare, `node:${bare}`];
  }),
);

const MANAGED_PACKAGE_NAMES = new Set(["convex", "neovex"]);

function collectModuleSpecifiers(source) {
  const specifiers = [];
  const patterns = [
    {
      kind: "import",
      regex: /\bimport\s+(?:[^"'()]+?\s+from\s+)?["']([^"']+)["']/g,
    },
    {
      kind: "dynamic import",
      regex: /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g,
    },
    {
      kind: "require",
      regex: /\brequire\s*\(\s*["']([^"']+)["']\s*\)/g,
    },
    {
      kind: "export",
      regex: /\bexport\s+[^"'()]+?\s+from\s+["']([^"']+)["']/g,
    },
  ];
  for (const { kind, regex } of patterns) {
    for (const match of source.matchAll(regex)) {
      specifiers.push({ kind, specifier: match[1] });
    }
  }
  return specifiers;
}

function isNodeBuiltinSpecifier(specifier) {
  if (BUILTIN_MODULES.has(specifier)) {
    return true;
  }
  const canonical = canonicalNodeSpecifier(specifier);
  return BUILTIN_MODULES.has(canonical) || BUILTIN_MODULES.has(`node:${canonical}`);
}

function canonicalNodeSpecifier(specifier) {
  return specifier.startsWith("node:") ? specifier.slice("node:".length) : specifier;
}

function isExternalPackageSpecifier(specifier) {
  const packageName = packageNameFromSpecifier(specifier);
  return packageName !== null && !MANAGED_PACKAGE_NAMES.has(packageName);
}

function packageNameFromSpecifier(specifier) {
  if (
    specifier.length === 0
    || specifier.startsWith(".")
    || specifier.startsWith("/")
    || specifier.startsWith("file:")
    || specifier.startsWith("data:")
    || specifier.startsWith("http:")
    || specifier.startsWith("https:")
    || isNodeBuiltinSpecifier(specifier)
  ) {
    return null;
  }

  const parts = specifier.split("/");
  if (specifier.startsWith("@")) {
    return parts.length >= 2 ? `${parts[0]}/${parts[1]}` : null;
  }
  return parts[0] || null;
}

export {
  canonicalNodeSpecifier,
  collectModuleSpecifiers,
  isExternalPackageSpecifier,
  isNodeBuiltinSpecifier,
  packageNameFromSpecifier,
};
