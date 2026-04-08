import type {
  ActionReference,
  MutationReference,
  Page,
  PaginatedQueryReference,
  QueryReference,
  InferArgs,
  InferResult,
} from "./internal/shared.ts";
import {
  createApiError,
  normalizeArgs,
  stripTrailingSlash,
  validateDeploymentUrl,
} from "./internal/shared.ts";

import { hasResolver } from "./browser-utils.ts";

type RequestTracker = {
  startedAt: Date;
  kind: "mutation" | "action" | "query";
};

export type FetchLike = typeof globalThis.fetch;
export type AuthTokenFetcher = (args: {
  forceRefreshToken: boolean;
}) => Promise<string | null | undefined>;
export type AuthChangeListener = (isAuthenticated: boolean) => void;

export class NeovexHttpClient {
  private readonly address: string;
  private fetchImpl?: FetchLike;
  private authToken?: string;
  private authTokenFetcher?: AuthTokenFetcher;
  private authChangeListener?: AuthChangeListener;
  private inflight = new Map<number, RequestTracker>();
  private nextRequestId = 1;

  constructor(
    address: string,
    options?: {
      skipDeploymentUrlCheck?: boolean;
      auth?: string;
      fetch?: FetchLike;
    },
  ) {
    if (options?.skipDeploymentUrlCheck !== true) {
      validateDeploymentUrl(address);
    }
    this.address = stripTrailingSlash(address);
    this.fetchImpl = options?.fetch;
    this.authToken = options?.auth;
  }

  get url() {
    return this.address;
  }

  setAuth(value: string | AuthTokenFetcher, onChange?: AuthChangeListener) {
    if (typeof value === "string") {
      this.authToken = value;
      this.authTokenFetcher = undefined;
      this.authChangeListener = onChange;
      this.reportAuthState(true);
      return;
    }

    this.authToken = undefined;
    this.authTokenFetcher = value;
    this.authChangeListener = onChange;
  }

  clearAuth() {
    this.authToken = undefined;
    this.authTokenFetcher = undefined;
    this.reportAuthState(false);
  }

  async query<Query extends QueryReference<any, any>>(
    query: Query,
    args?: InferArgs<Query>,
  ): Promise<InferResult<Query>> {
    const normalizedArgs = normalizeArgs(args);
    const body = hasResolver(query)
      ? { query: query.resolve(normalizedArgs) }
      : { name: query.name, args: normalizedArgs };
    return this.request("/query", body, "query");
  }

  async mutation<Mutation extends MutationReference<any, any>>(
    mutation: Mutation,
    args?: InferArgs<Mutation>,
  ): Promise<InferResult<Mutation>> {
    const normalizedArgs = normalizeArgs(args);
    const body = hasResolver(mutation)
      ? { mutation: mutation.resolve(normalizedArgs) }
      : { name: mutation.name, args: normalizedArgs };
    return this.request("/mutation", body, "mutation");
  }

  async action<Action extends ActionReference<any, any>>(
    action: Action,
    args?: InferArgs<Action>,
  ): Promise<InferResult<Action>> {
    const normalizedArgs = normalizeArgs(args);
    const body = hasResolver(action)
      ? { action: action.resolve(normalizedArgs) }
      : { name: action.name, args: normalizedArgs };
    return this.request("/action", body, "action");
  }

  async paginatedQuery<Query extends PaginatedQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query> | undefined,
    pageSize: number,
    cursor: string | null,
  ): Promise<Page<InferResult<Query>>> {
    const normalizedArgs = normalizeArgs(args);
    const body = {
      ...(hasResolver(query)
        ? {
            query: {
              query: query.resolve(normalizedArgs),
              page_size: pageSize,
              after: cursor,
            },
          }
        : {
            name: query.name,
            args: normalizedArgs,
            page_size: pageSize,
            cursor,
          }),
    };
    return this.request("/query/paginated", body, "query");
  }

  async scheduleAfter<Mutation extends MutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAfterMs: number,
  ): Promise<string> {
    const normalizedArgs = normalizeArgs(args);
    const body = hasResolver(mutation)
      ? { mutation: mutation.resolve(normalizedArgs), run_after_ms: runAfterMs }
      : { name: mutation.name, args: normalizedArgs, run_after_ms: runAfterMs };
    const response = await this.request<{ job_id: string }>(
      "/schedule/run_after",
      body,
      "mutation",
    );
    return response.job_id;
  }

  async scheduleAt<Mutation extends MutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAtMs: number,
  ): Promise<string> {
    const normalizedArgs = normalizeArgs(args);
    const body = hasResolver(mutation)
      ? { mutation: mutation.resolve(normalizedArgs), run_at_ms: runAtMs }
      : { name: mutation.name, args: normalizedArgs, run_at_ms: runAtMs };
    const response = await this.request<{ job_id: string }>(
      "/schedule/run_at",
      body,
      "mutation",
    );
    return response.job_id;
  }

  async cancelScheduledFunction(jobId: string): Promise<void> {
    await this.request<void>(`/schedule/${jobId}`, undefined, "mutation", "DELETE");
  }

  private async request<T>(
    suffix: string,
    body: unknown,
    kind: RequestTracker["kind"],
    method = "POST",
  ): Promise<T> {
    const requestId = this.nextRequestId++;
    this.inflight.set(requestId, { startedAt: new Date(), kind });
    try {
      const fetchImpl = this.fetchImpl ?? globalThis.fetch;
      let token = await this.getAuthToken(false);
      let response = await fetchImpl(`${this.address}${suffix}`, {
        method,
        headers: {
          ...(method !== "DELETE" ? { "Content-Type": "application/json" } : {}),
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });

      if (response.status === 401 && this.authTokenFetcher) {
        token = await this.getAuthToken(true);
        response = await fetchImpl(`${this.address}${suffix}`, {
          method,
          headers: {
            ...(method !== "DELETE" ? { "Content-Type": "application/json" } : {}),
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
          },
          ...(body === undefined ? {} : { body: JSON.stringify(body) }),
        });
      }

      const contentType = response.headers.get("content-type") ?? "";
      const payload = contentType.includes("application/json")
        ? await response.json()
        : await response.text();

      if (!response.ok) {
        if (response.status === 401) {
          this.reportAuthState(false);
        }
        throw createApiError(
          payload,
          `neovex request failed with ${response.status}`,
        );
      }

      this.reportAuthState(token !== null);
      return payload as T;
    } finally {
      this.inflight.delete(requestId);
    }
  }

  async getAuthToken(forceRefreshToken: boolean) {
    if (!this.authTokenFetcher) {
      return this.authToken ?? null;
    }

    const token = await this.authTokenFetcher({ forceRefreshToken });
    this.authToken = token ?? undefined;
    return token ?? null;
  }

  notifyAuthState(isAuthenticated: boolean) {
    this.reportAuthState(isAuthenticated);
  }

  canRefreshAuthToken() {
    return this.authTokenFetcher !== undefined;
  }

  private reportAuthState(isAuthenticated: boolean) {
    this.authChangeListener?.(isAuthenticated);
  }
}
