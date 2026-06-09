import {
  NimbusRestClient,
  type FetchLike,
  type RequestOptions,
} from "./transports/rest.ts";

export type NimbusCredential =
  | { kind: "bearer"; token: string }
  | { kind: "api_key"; apiKey: string }
  | {
      kind: "workload_identity";
      token: string;
      issuer?: string;
      audience?: string;
      subject?: string;
    };

export interface NimbusClientOptions {
  endpoint?: string;
  tenantId?: string;
  token?: string;
  apiKey?: string;
  credential?: NimbusCredential;
  fetch?: FetchLike;
  headers?: Record<string, string>;
}

export interface NimbusServiceSelector {
  name: string;
  tenantId?: string;
}

export type NimbusServiceWaitCondition = "ready" | "stopped" | "healthy";

export interface NimbusServiceLifecycleRequest extends NimbusServiceSelector {
  waitUntil?: NimbusServiceWaitCondition;
}

export interface NimbusServiceWaitRequest extends NimbusServiceSelector {
  until: NimbusServiceWaitCondition;
  timeoutMs?: number;
  intervalMs?: number;
}

export interface NimbusServiceEndpoint {
  name: string;
  protocol: string;
  host: string;
  port: number;
}

export interface NimbusService {
  name: string;
  tenantId?: string;
  state?: string;
  lifecycleState?: string;
  readiness?: string;
  health?: string;
  sandboxId?: string;
  backend?: string;
  endpoints?: NimbusServiceEndpoint[];
  status?: string;
  [key: string]: unknown;
}

type ProcessLike = {
  env?: Record<string, string | undefined>;
};

type LocalCredentialFile = {
  endpoint?: unknown;
  token?: unknown;
  access_token?: unknown;
  api_key?: unknown;
  apiKey?: unknown;
  credential?: {
    kind?: unknown;
    type?: unknown;
    token?: unknown;
    access_token?: unknown;
    api_key?: unknown;
    apiKey?: unknown;
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

type ControlPlaneRequest = <T = unknown>(
  path: string,
  options?: RequestOptions,
) => Promise<T>;

export class Nimbus {
  readonly services: NimbusServices;
  readonly tenantId?: string;

  readonly #options: NimbusClientOptions;
  #clientPromise: Promise<NimbusRestClient> | null = null;

  constructor(options: NimbusClientOptions = {}) {
    this.#options = { ...options };
    this.tenantId = stringOrUndefined(options.tenantId);
    this.services = new NimbusServices(
      this,
      this.#controlPlaneRequest.bind(this),
    );
  }

  static async defaultClient(options: NimbusClientOptions = {}): Promise<Nimbus> {
    const client = new Nimbus(options);
    await client.#resolveRestClient();
    return client;
  }

  async #controlPlaneRequest<T = unknown>(
    path: string,
    options: RequestOptions = {},
  ): Promise<T> {
    const client = await this.#resolveRestClient();
    return client.request<T>(path, options);
  }

  async #resolveRestClient(): Promise<NimbusRestClient> {
    this.#clientPromise ??= createDefaultRestClient(this.#options);
    return this.#clientPromise;
  }
}

class NimbusServices {
  constructor(
    private readonly client: Nimbus,
    private readonly sendControlPlaneRequest: ControlPlaneRequest,
  ) {}

  async start(input: NimbusServiceLifecycleRequest): Promise<NimbusService> {
    const service = await this.lifecycle("start", input);
    if (!input.waitUntil) return service;
    return this.wait({
      name: input.name,
      tenantId: input.tenantId,
      until: input.waitUntil,
    });
  }

  async stop(input: NimbusServiceLifecycleRequest): Promise<NimbusService> {
    const service = await this.lifecycle("stop", input);
    if (!input.waitUntil) return service;
    return this.wait({
      name: input.name,
      tenantId: input.tenantId,
      until: input.waitUntil,
    });
  }

  async restart(input: NimbusServiceLifecycleRequest): Promise<NimbusService> {
    const service = await this.lifecycle("restart", input);
    if (!input.waitUntil) return service;
    return this.wait({
      name: input.name,
      tenantId: input.tenantId,
      until: input.waitUntil,
    });
  }

  get(selector: NimbusServiceSelector): Promise<NimbusService> {
    return this.sendControlPlaneRequest(serviceResourcePath(this.client, selector), {
      method: "GET",
    });
  }

  async wait(input: NimbusServiceWaitRequest): Promise<NimbusService> {
    const timeoutMs = positiveFiniteNumber(input.timeoutMs, 30_000, "timeoutMs");
    const intervalMs = positiveFiniteNumber(input.intervalMs, 250, "intervalMs");
    const deadline = Date.now() + timeoutMs;
    let latest: NimbusService | null = null;

    while (Date.now() <= deadline) {
      latest = await this.get(input);
      if (serviceMatchesCondition(latest, input.until)) {
        return latest;
      }
      await sleep(intervalMs);
    }

    const observed = latest
      ? normalizeStatusString(latest.readiness)
        ?? normalizeStatusString(latest.health)
        ?? normalizeStatusString(latest.lifecycleState)
        ?? normalizeStatusString(latest.state)
        ?? "unknown"
      : "unknown";
    throw new Error(
      `Nimbus service ${input.name} did not reach ${input.until} within ${timeoutMs}ms; last observed status was ${observed}.`,
    );
  }

  private lifecycle(
    verb: "start" | "stop" | "restart",
    input: NimbusServiceLifecycleRequest,
  ): Promise<NimbusService> {
    return this.sendControlPlaneRequest(serviceResourcePath(this.client, input, `/${verb}`), {
      method: "POST",
    });
  }
}

async function createDefaultRestClient(options: NimbusClientOptions): Promise<NimbusRestClient> {
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
  if (options.apiKey) return { kind: "api_key", apiKey: options.apiKey };
  return null;
}

function normalizeEnvCredential(env: Record<string, string | undefined>): NimbusCredential | null {
  const token = stringOrUndefined(env.NIMBUS_TOKEN)
    ?? stringOrUndefined(env.NIMBUS_BEARER_TOKEN);
  if (token) return { kind: "bearer", token };

  const apiKey = stringOrUndefined(env.NIMBUS_API_KEY);
  if (apiKey) return { kind: "api_key", apiKey };

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
    const nestedApiKey = stringOrUndefined(nested.apiKey) ?? stringOrUndefined(nested.api_key);
    if ((kind === "api_key" || kind === "apiKey") && nestedApiKey) {
      return { kind: "api_key", apiKey: nestedApiKey };
    }
  }

  const token = stringOrUndefined(file.token) ?? stringOrUndefined(file.access_token);
  if (token) return { kind: "bearer", token };

  const apiKey = stringOrUndefined(file.apiKey) ?? stringOrUndefined(file.api_key);
  if (apiKey) return { kind: "api_key", apiKey };

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
    case "api_key":
      return { "X-Nimbus-Api-Key": credential.apiKey };
    case "bearer":
    case "workload_identity":
      return { Authorization: `Bearer ${credential.token}` };
  }
}

function getEnv(): Record<string, string | undefined> {
  return ((globalThis as typeof globalThis & { process?: ProcessLike }).process?.env ?? {});
}

function clientTenantId(client: Nimbus): string | undefined {
  return client.tenantId;
}

function serviceResourcePath(
  client: Nimbus,
  selector: NimbusServiceSelector,
  suffix = "",
): string {
  const tenantId = resolveTenantId(client, selector, "service request");
  const tenant = encodeResourceName(tenantId, "tenant");
  const name = encodeResourceName(selector.name, "service");
  return `/api/tenants/${tenant}/services/${name}${suffix}`;
}

function resolveTenantId(
  client: Nimbus,
  selector: NimbusServiceSelector,
  context: string,
): string {
  const tenantId = stringOrUndefined(selector.tenantId) ?? clientTenantId(client);
  if (!tenantId) {
    throw new Error(
      `Nimbus ${context} requires a tenantId. Pass tenantId in the service call or construct the client with new Nimbus({ tenantId }).`,
    );
  }
  return tenantId;
}

function encodeResourceName(value: string, label: string): string {
  const trimmed = value.trim();
  if (!trimmed) throw new Error(`Nimbus ${label} name must not be empty`);
  return encodeURIComponent(trimmed);
}

function positiveFiniteNumber(value: unknown, fallback: number, label: string): number {
  if (value === undefined) return fallback;
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return value;
  }
  throw new Error(`Nimbus service wait ${label} must be a positive finite number.`);
}

function serviceMatchesCondition(
  service: NimbusService,
  condition: NimbusServiceWaitCondition,
): boolean {
  const state = normalizeStatusString(service.lifecycleState)
    ?? normalizeStatusString(service.state)
    ?? normalizeStatusString(service.status);
  const readiness = normalizeStatusString(service.readiness);
  const health = normalizeStatusString(service.health);

  switch (condition) {
    case "ready":
      return readiness === "ready" || state === "ready";
    case "stopped":
      return state === "stopped" || readiness === "stopped";
    case "healthy":
      return health === "healthy";
  }
}

function normalizeStatusString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value.toLowerCase() : undefined;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function stringOrUndefined(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function stripTrailingSlash(value: string): string {
  return value.endsWith("/") ? value.slice(0, -1) : value;
}
