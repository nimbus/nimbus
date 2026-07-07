import { NimbusRestClient } from "../transports/rest.ts";
import type { NimbusClientOptions, NimbusCredential } from "./types.ts";

type ProcessLike = {
  env?: Record<string, string | undefined>;
};

type LocalCredentialFile = {
  endpoint?: unknown;
  token?: unknown;
  access_token?: unknown;
  credential?: {
    kind?: unknown;
    type?: unknown;
    token?: unknown;
    access_token?: unknown;
    issuer?: unknown;
    audience?: unknown;
    subject?: unknown;
  };
};

type FsPromises = {
  readFile(path: string, encoding: "utf8"): Promise<string>;
};

const dynamicImport = new Function(
  "specifier",
  "return import(specifier)",
) as (specifier: string) => Promise<unknown>;

export async function createDefaultRestClient(options: NimbusClientOptions): Promise<NimbusRestClient> {
  const env = getEnv();
  const explicitCredential = normalizeExplicitCredential(options);
  const envCredential = normalizeEnvCredential(env);
  const explicitEndpoint = stringOrUndefined(options.endpoint);
  const envEndpoint = stringOrUndefined(env.NIMBUS_ENDPOINT);
  const localCredentials =
    (!explicitEndpoint && !envEndpoint) || (!explicitCredential && !envCredential)
      ? await readLocalCredentialFile(env)
      : null;
  const endpoint = explicitEndpoint
    ?? envEndpoint
    ?? stringOrUndefined(localCredentials?.endpoint);
  if (!endpoint) {
    throw new Error(
      "Nimbus endpoint discovery failed. Set new Nimbus({ endpoint }), NIMBUS_ENDPOINT, or endpoint in ~/.config/nimbus/application_default_credentials.json.",
    );
  }

  const credential =
    explicitCredential
    ?? envCredential
    ?? normalizeLocalCredential(localCredentials)
    ?? await resolveWorkloadIdentityCredential(env);
  if (!credential) {
    throw new Error(
      "Nimbus credential discovery failed. Set new Nimbus({ token }), NIMBUS_TOKEN, a local Nimbus application_default_credentials.json file, or a workload identity token.",
    );
  }

  return new NimbusRestClient(endpoint, {
    fetch: options.fetch,
    headers: {
      ...headersForCredential(credential),
      ...(options.headers ?? {}),
    },
  });
}

function normalizeExplicitCredential(options: NimbusClientOptions): NimbusCredential | null {
  if (options.credential) return options.credential;
  if (options.token) return { kind: "bearer", token: options.token };
  return null;
}

function normalizeEnvCredential(env: Record<string, string | undefined>): NimbusCredential | null {
  const token = stringOrUndefined(env.NIMBUS_TOKEN)
    ?? stringOrUndefined(env.NIMBUS_BEARER_TOKEN);
  if (token) return { kind: "bearer", token };

  const workloadToken = stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_TOKEN);
  if (workloadToken) {
    return {
      kind: "workload_identity",
      token: workloadToken,
      issuer: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_ISSUER),
      audience: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_AUDIENCE),
      subject: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_SUBJECT),
    };
  }

  return null;
}

function normalizeLocalCredential(file: LocalCredentialFile | null): NimbusCredential | null {
  if (!file) return null;

  const nested = file.credential;
  if (nested) {
    const kind = stringOrUndefined(nested.kind) ?? stringOrUndefined(nested.type);
    const token = stringOrUndefined(nested.token) ?? stringOrUndefined(nested.access_token);
    if (kind === "workload_identity" && token) {
      return {
        kind: "workload_identity",
        token,
        issuer: stringOrUndefined(nested.issuer),
        audience: stringOrUndefined(nested.audience),
        subject: stringOrUndefined(nested.subject),
      };
    }
    if ((kind === "bearer" || kind === "access_token") && token) {
      return { kind: "bearer", token };
    }
  }

  const token = stringOrUndefined(file.token) ?? stringOrUndefined(file.access_token);
  if (token) return { kind: "bearer", token };

  return null;
}

async function resolveWorkloadIdentityCredential(
  env: Record<string, string | undefined>,
): Promise<NimbusCredential | null> {
  const tokenFile = stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_TOKEN_FILE);
  if (!tokenFile) return null;

  const token = (await readTextFileIfExists(tokenFile))?.trim();
  if (!token) return null;

  return {
    kind: "workload_identity",
    token,
    issuer: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_ISSUER),
    audience: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_AUDIENCE),
    subject: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_SUBJECT),
  };
}

async function readLocalCredentialFile(
  env: Record<string, string | undefined>,
): Promise<LocalCredentialFile | null> {
  const explicitPath = stringOrUndefined(env.NIMBUS_APPLICATION_CREDENTIALS);
  const defaultPath = defaultCredentialFilePath(env);
  const raw = await readTextFileIfExists(explicitPath ?? defaultPath);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as LocalCredentialFile;
  } catch (error) {
    throw new Error(
      `Nimbus credential discovery failed: ${(explicitPath ?? defaultPath) || "credential file"} is not valid JSON: ${(error as Error).message}`,
    );
  }
}

async function readTextFileIfExists(filePath: string | undefined): Promise<string | null> {
  if (!filePath) return null;
  const fs = await dynamicImport("node:fs/promises") as FsPromises;
  try {
    return await fs.readFile(filePath, "utf8");
  } catch (error) {
    if (
      error
      && typeof error === "object"
      && "code" in error
      && error.code === "ENOENT"
    ) {
      return null;
    }
    throw error;
  }
}

function defaultCredentialFilePath(env: Record<string, string | undefined>): string | undefined {
  const configHome = stringOrUndefined(env.XDG_CONFIG_HOME);
  if (configHome) return `${stripTrailingSlash(configHome)}/nimbus/application_default_credentials.json`;

  const home = stringOrUndefined(env.HOME) ?? stringOrUndefined(env.USERPROFILE);
  return home ? `${stripTrailingSlash(home)}/.config/nimbus/application_default_credentials.json` : undefined;
}

function headersForCredential(credential: NimbusCredential): Record<string, string> {
  switch (credential.kind) {
    case "bearer":
    case "workload_identity":
      return { Authorization: `Bearer ${credential.token}` };
  }
}

function getEnv(): Record<string, string | undefined> {
  return ((globalThis as typeof globalThis & { process?: ProcessLike }).process?.env ?? {});
}

export function stringOrUndefined(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function stripTrailingSlash(value: string): string {
  return value.endsWith("/") ? value.slice(0, -1) : value;
}
