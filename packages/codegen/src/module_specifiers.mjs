import { builtinModules } from "node:module";

const BUILTIN_MODULES = new Set(
  builtinModules.flatMap((specifier) => {
    const bare = specifier.startsWith("node:") ? specifier.slice("node:".length) : specifier;
    return [bare, `node:${bare}`];
  }),
);

const MANAGED_PACKAGE_NAMES = new Set(["convex", "@nimbus/nimbus"]);
const TENANT_BUNDLE_OPERATOR_ONLY_SPECIFIERS = new Set([
  "nimbus/rest",
]);
const TENANT_BUNDLE_OPERATOR_CREDENTIAL_PATTERNS = Object.freeze([
  {
    label: "LocalAdminTokenRecord",
    regex: /\bLocalAdminTokenRecord\b/,
  },
  {
    label: "NIMBUS_LOCAL_ADMIN_TOKEN",
    regex: /\bNIMBUS_LOCAL_ADMIN_TOKEN\b/,
  },
  {
    label: "NIMBUS_DEPLOY_TOKEN",
    regex: /\bNIMBUS_DEPLOY_TOKEN\b/,
  },
  {
    label: "NIMBUS_TOKEN",
    regex: /\bNIMBUS_TOKEN\b/,
  },
  {
    label: "NIMBUS_BEARER_TOKEN",
    regex: /\bNIMBUS_BEARER_TOKEN\b/,
  },
]);

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

function isTenantBundleOperatorOnlySpecifier(specifier) {
  return TENANT_BUNDLE_OPERATOR_ONLY_SPECIFIERS.has(specifier)
    || specifier === "@nimbus/nimbus/transports"
    || specifier.startsWith("@nimbus/nimbus/transports/")
    || specifier.startsWith("nimbus/transports/")
    || specifier.startsWith("nimbus/rest/")
    || specifier === "nimbus/transports";
}

function collectTenantBundleAdmissionViolations(source, { file = "tenant module" } = {}) {
  const violations = [];
  for (const { kind, specifier } of collectModuleSpecifiers(source)) {
    if (isTenantBundleOperatorOnlySpecifier(specifier)) {
      violations.push({
        file,
        kind,
        specifier,
        reason:
          "tenant bundle admission rejects operator-only low-level Nimbus transport imports; use the high-level @nimbus/nimbus SDK with workload identity",
      });
    }
  }
  for (const { label, regex } of TENANT_BUNDLE_OPERATOR_CREDENTIAL_PATTERNS) {
    if (regex.test(source)) {
      violations.push({
        file,
        kind: "operator credential",
        specifier: label,
        reason:
          "tenant bundle admission rejects packaged operator credential material; tenant code must use workload identity, not local-admin tokens or static control-plane credentials",
      });
    }
  }
  return violations;
}

function assertTenantBundleAdmission(source, options = {}) {
  const violations = collectTenantBundleAdmissionViolations(source, options);
  if (violations.length === 0) {
    return;
  }
  const details = violations
    .map((violation) =>
      `${violation.file}: ${violation.kind} ${JSON.stringify(violation.specifier)} -- ${violation.reason}`
    )
    .join("; ");
  throw new Error(`tenant bundle admission failed: ${details}`);
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
  assertTenantBundleAdmission,
  collectTenantBundleAdmissionViolations,
  collectModuleSpecifiers,
  isExternalPackageSpecifier,
  isNodeBuiltinSpecifier,
  isTenantBundleOperatorOnlySpecifier,
  packageNameFromSpecifier,
};
