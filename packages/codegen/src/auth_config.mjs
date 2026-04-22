import fs from "node:fs/promises";
import path from "node:path";

import { build } from "esbuild";

const EMPTY_AUTH_CONFIG = Object.freeze({ providers: [] });
const AUTH_CONFIG_CANDIDATES = ["auth.config.ts", "auth.config.js"];

async function loadAuthConfig(convexDir) {
  const authConfigPath = await findAuthConfigPath(convexDir);
  if (authConfigPath === null) {
    return EMPTY_AUTH_CONFIG;
  }

  const bundledSource = await bundleAuthConfig(authConfigPath);
  const moduleUrl = `data:text/javascript;base64,${Buffer.from(bundledSource).toString("base64")}`;
  const module = await import(moduleUrl);
  return normalizeAuthConfig(module.default, authConfigPath);
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

async function bundleAuthConfig(authConfigPath) {
  const result = await build({
    entryPoints: [authConfigPath],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "node20",
    logLevel: "silent",
    plugins: [
      {
        name: "convex-auth-config-stubs",
        setup(build) {
          build.onResolve({ filter: /^(convex|neovex)\/server$/ }, () => ({
            path: "convex-server-stub",
            namespace: "convex-auth-config",
          }));
          build.onLoad(
            { filter: /^convex-server-stub$/, namespace: "convex-auth-config" },
            () => ({
              contents: "export {};",
              loader: "js",
            }),
          );
        },
      },
    ],
  });
  const outputFile = result.outputFiles?.[0];
  if (!outputFile) {
    throw new Error(`failed to bundle ${relativeForDisplay(authConfigPath)}`);
  }
  return outputFile.text;
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
