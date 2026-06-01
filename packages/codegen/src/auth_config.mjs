import fs from "node:fs/promises";
import path from "node:path";

import { evaluateModuleDefaultExport } from "./compile_time_interpreter.mjs";

// auth.config is evaluated in-binary by the compile-time TypeScript AST
// interpreter — the same path used for schema/server extraction. It statically
// evaluates the module's `export default` (object/array/string/number/boolean
// literals, hoisted `const`s, template strings, and `process.env.*` reads via
// the opt-in global below) without esbuild bundling or a dynamic `import()`, so
// default Convex codegen — including auth.config — runs in the in-binary V8
// tooling runtime with no external Node (BPD4/BPD7).

const EMPTY_AUTH_CONFIG = Object.freeze({ providers: [] });
const AUTH_CONFIG_CANDIDATES = ["auth.config.ts", "auth.config.js"];

async function loadAuthConfig(convexDir) {
  const authConfigPath = await findAuthConfigPath(convexDir);
  if (authConfigPath === null) {
    return EMPTY_AUTH_CONFIG;
  }

  const source = await fs.readFile(authConfigPath, "utf8");
  const evaluated = evaluateAuthConfigDefaultExport(source, authConfigPath);
  return normalizeAuthConfig(evaluated, authConfigPath);
}

function evaluateAuthConfigDefaultExport(source, filePath) {
  try {
    return evaluateModuleDefaultExport(source, {
      filePath: relativeForDisplay(filePath),
      // Mirror the prior esbuild+import behavior: `process.env.*` reads resolve
      // against the codegen process environment at codegen time.
      globals: { process: { env: { ...process.env } } },
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`failed to evaluate ${relativeForDisplay(filePath)}: ${message}`);
  }
}

async function findAuthConfigPath(convexDir) {
  const foundPaths = [];
  for (const candidate of AUTH_CONFIG_CANDIDATES) {
    const candidatePath = path.join(convexDir, candidate);
    try {
      const stats = await fs.stat(candidatePath);
      if (stats.isFile()) {
        foundPaths.push(candidatePath);
      }
    } catch (error) {
      if (error?.code !== "ENOENT") {
        throw error;
      }
    }
  }
  if (foundPaths.length > 1) {
    throw new Error(
      `Found both ${foundPaths[1]} and ${foundPaths[0]}, choose one.`,
    );
  }
  return foundPaths[0] ?? null;
}

function normalizeAuthConfig(rawConfig, filePath) {
  if (
    rawConfig === null
    || typeof rawConfig !== "object"
    || Array.isArray(rawConfig)
  ) {
    throw new Error(
      `${relativeForDisplay(filePath)} must export a default auth config object`,
    );
  }

  const { providers } = rawConfig;
  if (!Array.isArray(providers)) {
    throw new Error(
      `${relativeForDisplay(filePath)} must export { providers: [...] }`,
    );
  }

  return {
    providers: providers.map((provider) => normalizeAuthProvider(provider, filePath)),
  };
}

function normalizeAuthProvider(provider, filePath) {
  if (provider === null || typeof provider !== "object" || Array.isArray(provider)) {
    throw new Error(
      `${relativeForDisplay(filePath)} auth providers must be objects`,
    );
  }

  if (provider.type === undefined) {
    if (
      typeof provider.domain !== "string"
      || provider.domain.length === 0
      || typeof provider.applicationID !== "string"
      || provider.applicationID.length === 0
    ) {
      throw new Error(
        `${relativeForDisplay(filePath)} OIDC providers require domain and applicationID`,
      );
    }
    return {
      domain: provider.domain,
      applicationID: provider.applicationID,
    };
  }

  if (provider.type !== "customJwt") {
    throw new Error(
      `${relativeForDisplay(filePath)} auth provider type "${String(provider.type)}" is not supported`,
    );
  }
  if (typeof provider.issuer !== "string" || provider.issuer.length === 0) {
    throw new Error(
      `${relativeForDisplay(filePath)} customJwt providers require issuer`,
    );
  }
  if (typeof provider.jwks !== "string" || provider.jwks.length === 0) {
    throw new Error(
      `${relativeForDisplay(filePath)} customJwt providers require jwks`,
    );
  }
  if (provider.algorithm !== "RS256" && provider.algorithm !== "ES256") {
    throw new Error(
      `${relativeForDisplay(filePath)} customJwt providers require algorithm "RS256" or "ES256"`,
    );
  }
  if (
    provider.applicationID !== undefined
    && (typeof provider.applicationID !== "string" || provider.applicationID.length === 0)
  ) {
    throw new Error(
      `${relativeForDisplay(filePath)} customJwt applicationID must be a non-empty string when provided`,
    );
  }

  return {
    type: "customJwt",
    issuer: provider.issuer,
    jwks: provider.jwks,
    algorithm: provider.algorithm,
    ...(provider.applicationID ? { applicationID: provider.applicationID } : {}),
  };
}

function relativeForDisplay(filePath) {
  return path.relative(process.cwd(), filePath);
}

export { loadAuthConfig };
