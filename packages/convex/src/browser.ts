import {
  NimbusClient,
  NimbusHttpClient,
  NimbusReactClient,
} from "nimbus/browser";
import type {
  AuthTokenFetcher,
  ConnectionState,
  Unsubscribe,
  WebSocketConstructor,
} from "nimbus/browser";
import type {
  ConvexActionReference,
  ConvexMutationReference,
  ConvexPage,
  ConvexPaginatedQueryReference,
  ConvexQueryReference,
  InferArgs,
  InferResult,
  JsonValue,
} from "./internal/shared.ts";
import {
  makeActionReference,
  makeMutationReference,
  makeQueryReference,
} from "./internal/shared.ts";

export {
  defineAction,
  defineMutation,
  definePaginatedQuery,
  defineQuery,
  makeActionReference,
  makeMutationReference,
  makePaginatedQueryReference,
  makeQueryReference,
} from "./internal/shared.ts";
export type {
  AuthTokenFetcher,
  ConnectionState,
  Unsubscribe,
  WebSocketConstructor,
} from "nimbus/browser";

type FetchLike = typeof globalThis.fetch;

type ConvexHttpClientOptions = {
  skipConvexDeploymentUrlCheck?: boolean;
  auth?: string;
  fetch?: FetchLike;
};

type ConvexClientOptions = ConvexHttpClientOptions & {
  disabled?: boolean;
  authRefreshTokenLeewaySeconds?: number;
  webSocket?: WebSocketConstructor;
};

const ANY_API_PATH = Symbol("convex.anyApiPath");

type AnyApiRef = {
  readonly [ANY_API_PATH]: readonly string[];
};

export type AnyApi = {
  readonly [key: string]: AnyApi;
};

type NamedArgs = Record<string, JsonValue>;

function makeAnyApiProxy(path: readonly string[]): AnyApi {
  const state = { [ANY_API_PATH]: path } as AnyApiRef;
  return new Proxy(state as AnyApiRef & AnyApi, {
    get(target, prop) {
      if (prop === ANY_API_PATH) {
        return target[ANY_API_PATH];
      }
      if (
        prop === "then" ||
        prop === "toJSON" ||
        prop === "toString" ||
        prop === "valueOf" ||
        prop === "inspect" ||
        typeof prop !== "string"
      ) {
        return undefined;
      }
      return makeAnyApiProxy([...target[ANY_API_PATH], prop]);
    },
  }) as AnyApi;
}

export const anyApi = makeAnyApiProxy([]);

function isAnyApiReference(input: unknown): input is AnyApiRef {
  return (
    typeof input === "object" &&
    input !== null &&
    ANY_API_PATH in (input as Record<PropertyKey, unknown>)
  );
}

function coerceName(input: string | AnyApi) {
  if (typeof input === "string") {
    return input;
  }
  if (!isAnyApiReference(input)) {
    throw new Error("Unsupported Convex function reference.");
  }
  const path = input[ANY_API_PATH];
  if (path.length < 2) {
    throw new Error("anyApi references must include at least a module and export name.");
  }
  return `${path.slice(0, -1).join("/")}:${path.at(-1)}`;
}

function coerceQueryReference<Query extends ConvexQueryReference<any, any>>(
  input: Query | string | AnyApi,
): Query | ConvexQueryReference<NamedArgs, unknown> {
  return typeof input === "string" || isAnyApiReference(input)
    ? makeQueryReference<NamedArgs>(coerceName(input))
    : (input as Query);
}

function coerceMutationReference<Mutation extends ConvexMutationReference<any, any>>(
  input: Mutation | string | AnyApi,
): Mutation | ConvexMutationReference<NamedArgs, unknown> {
  return typeof input === "string" || isAnyApiReference(input)
    ? makeMutationReference<NamedArgs>(coerceName(input))
    : (input as Mutation);
}

function coerceActionReference<Action extends ConvexActionReference<any, any>>(
  input: Action | string | AnyApi,
): Action | ConvexActionReference<NamedArgs, unknown> {
  return typeof input === "string" || isAnyApiReference(input)
    ? makeActionReference<NamedArgs>(coerceName(input))
    : (input as Action);
}

function assertNamedLiveQueryOptions(query: string | AnyApi, options?: { pageSize?: number; cursor?: string | null }) {
  if (options?.pageSize !== undefined || options?.cursor !== undefined) {
    throw new Error(
      `String refs and anyApi do not support paginated live queries yet: ${coerceName(query)}`,
    );
  }
}

function withConvexDeploymentUrlCheck<T extends { skipConvexDeploymentUrlCheck?: boolean }>(
  options: T | undefined,
) {
  const { skipConvexDeploymentUrlCheck, ...rest } = options ?? {};
  return {
    ...rest,
    ...(skipConvexDeploymentUrlCheck === undefined
      ? {}
      : { skipDeploymentUrlCheck: skipConvexDeploymentUrlCheck }),
  };
}

export class ConvexHttpClient extends NimbusHttpClient {
  constructor(address: string, options: ConvexHttpClientOptions = {}) {
    super(address, withConvexDeploymentUrlCheck(options));
  }

  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query,
    args?: InferArgs<Query>,
  ): Promise<InferResult<Query>>;
  async query(query: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query | string | AnyApi,
    args?: InferArgs<Query> | NamedArgs,
  ): Promise<InferResult<Query> | unknown> {
    return super.query(
      coerceQueryReference(query) as ConvexQueryReference<any, any>,
      args as InferArgs<Query>,
    ) as Promise<
      InferResult<Query> | unknown
    >;
  }

  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args?: InferArgs<Mutation>,
  ): Promise<InferResult<Mutation>>;
  async mutation(mutation: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args?: InferArgs<Mutation> | NamedArgs,
  ): Promise<InferResult<Mutation> | unknown> {
    return super.mutation(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation>,
    ) as Promise<
      InferResult<Mutation> | unknown
    >;
  }

  async action<Action extends ConvexActionReference<any, any>>(
    action: Action,
    args?: InferArgs<Action>,
  ): Promise<InferResult<Action>>;
  async action(action: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async action<Action extends ConvexActionReference<any, any>>(
    action: Action | string | AnyApi,
    args?: InferArgs<Action> | NamedArgs,
  ): Promise<InferResult<Action> | unknown> {
    return super.action(
      coerceActionReference(action) as ConvexActionReference<any, any>,
      args as InferArgs<Action>,
    ) as Promise<
      InferResult<Action> | unknown
    >;
  }

  async paginatedQuery<Query extends ConvexPaginatedQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query> | undefined,
    pageSize: number,
    cursor: string | null,
  ): Promise<ConvexPage<InferResult<Query>>> {
    return super.paginatedQuery(query, args, pageSize, cursor) as Promise<
      ConvexPage<InferResult<Query>>
    >;
  }

  async scheduleAfter<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAfterMs: number,
  ): Promise<string>;
  async scheduleAfter(
    mutation: string | AnyApi,
    args: NamedArgs | undefined,
    runAfterMs: number,
  ): Promise<string>;
  async scheduleAfter<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args: InferArgs<Mutation> | NamedArgs | undefined,
    runAfterMs: number,
  ): Promise<string> {
    return super.scheduleAfter(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation> | undefined,
      runAfterMs,
    );
  }

  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAtMs: number,
  ): Promise<string>;
  async scheduleAt(
    mutation: string | AnyApi,
    args: NamedArgs | undefined,
    runAtMs: number,
  ): Promise<string>;
  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args: InferArgs<Mutation> | NamedArgs | undefined,
    runAtMs: number,
  ): Promise<string> {
    return super.scheduleAt(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation> | undefined,
      runAtMs,
    );
  }
}

export class ConvexClient extends NimbusClient {
  constructor(address: string, options: ConvexClientOptions = {}) {
    super(address, withConvexDeploymentUrlCheck(options));
  }

  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query,
    args?: InferArgs<Query>,
  ): Promise<InferResult<Query>>;
  async query(query: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query | string | AnyApi,
    args?: InferArgs<Query> | NamedArgs,
  ): Promise<InferResult<Query> | unknown> {
    return super.query(
      coerceQueryReference(query) as ConvexQueryReference<any, any>,
      args as InferArgs<Query>,
    ) as Promise<
      InferResult<Query> | unknown
    >;
  }

  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args?: InferArgs<Mutation>,
  ): Promise<InferResult<Mutation>>;
  async mutation(mutation: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args?: InferArgs<Mutation> | NamedArgs,
  ): Promise<InferResult<Mutation> | unknown> {
    return super.mutation(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation>,
    ) as Promise<
      InferResult<Mutation> | unknown
    >;
  }

  async action<Action extends ConvexActionReference<any, any>>(
    action: Action,
    args?: InferArgs<Action>,
  ): Promise<InferResult<Action>>;
  async action(action: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async action<Action extends ConvexActionReference<any, any>>(
    action: Action | string | AnyApi,
    args?: InferArgs<Action> | NamedArgs,
  ): Promise<InferResult<Action> | unknown> {
    return super.action(
      coerceActionReference(action) as ConvexActionReference<any, any>,
      args as InferArgs<Action>,
    ) as Promise<
      InferResult<Action> | unknown
    >;
  }

  async paginatedQuery<Query extends ConvexPaginatedQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query> | undefined,
    pageSize: number,
    cursor: string | null,
  ): Promise<ConvexPage<InferResult<Query>>> {
    return super.paginatedQuery(query, args, pageSize, cursor) as Promise<
      ConvexPage<InferResult<Query>>
    >;
  }

  async scheduleAfter<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAfterMs: number,
  ): Promise<string>;
  async scheduleAfter(
    mutation: string | AnyApi,
    args: NamedArgs | undefined,
    runAfterMs: number,
  ): Promise<string>;
  async scheduleAfter<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args: InferArgs<Mutation> | NamedArgs | undefined,
    runAfterMs: number,
  ): Promise<string> {
    return super.scheduleAfter(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation> | undefined,
      runAfterMs,
    );
  }

  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAtMs: number,
  ): Promise<string>;
  async scheduleAt(
    mutation: string | AnyApi,
    args: NamedArgs | undefined,
    runAtMs: number,
  ): Promise<string>;
  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args: InferArgs<Mutation> | NamedArgs | undefined,
    runAtMs: number,
  ): Promise<string> {
    return super.scheduleAt(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation> | undefined,
      runAtMs,
    );
  }

  onUpdate<Query extends ConvexQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query>,
    callback: (result: InferResult<Query>) => unknown,
    onError?: (error: Error) => unknown,
  ): Unsubscribe<InferResult<Query>>;
  onUpdate(
    query: string | AnyApi,
    args: NamedArgs,
    callback: (result: unknown) => unknown,
    onError?: (error: Error) => unknown,
  ): Unsubscribe<unknown>;
  onUpdate<Query extends ConvexQueryReference<any, any> | ConvexPaginatedQueryReference<any, any>>(
    query: Query | string | AnyApi,
    args: InferArgs<Query> | NamedArgs,
    callback: (
      result: Query extends ConvexPaginatedQueryReference<any, infer Item>
        ? Item[]
        : InferResult<Query>,
    ) => unknown,
    onError?: (error: Error) => unknown,
    options?: { pageSize?: number; cursor?: string | null },
  ): Unsubscribe<
    Query extends ConvexPaginatedQueryReference<any, infer Item>
      ? Item[]
      : InferResult<Query>
  > {
    if (typeof query === "string" || isAnyApiReference(query)) {
      assertNamedLiveQueryOptions(query, options);
      return super.onUpdate(
        coerceQueryReference(query) as ConvexQueryReference<any, any>,
        args as NamedArgs,
        callback as (result: unknown) => unknown,
        onError,
      ) as Unsubscribe<
        Query extends ConvexPaginatedQueryReference<any, infer Item>
          ? Item[]
          : InferResult<Query>
      >;
    }
    return super.onUpdate(query as Query, args as InferArgs<Query>, callback, onError, options) as Unsubscribe<
      Query extends ConvexPaginatedQueryReference<any, infer Item>
        ? Item[]
        : InferResult<Query>
    >;
  }
}

export class ConvexReactClient extends NimbusReactClient {
  constructor(address: string, options: ConvexClientOptions = {}) {
    super(address, withConvexDeploymentUrlCheck(options));
  }

  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query,
    args?: InferArgs<Query>,
  ): Promise<InferResult<Query>>;
  async query(query: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query | string | AnyApi,
    args?: InferArgs<Query> | NamedArgs,
  ): Promise<InferResult<Query> | unknown> {
    return super.query(
      coerceQueryReference(query) as ConvexQueryReference<any, any>,
      args as InferArgs<Query>,
    ) as Promise<
      InferResult<Query> | unknown
    >;
  }

  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args?: InferArgs<Mutation>,
  ): Promise<InferResult<Mutation>>;
  async mutation(mutation: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args?: InferArgs<Mutation> | NamedArgs,
  ): Promise<InferResult<Mutation> | unknown> {
    return super.mutation(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation>,
    ) as Promise<
      InferResult<Mutation> | unknown
    >;
  }

  async action<Action extends ConvexActionReference<any, any>>(
    action: Action,
    args?: InferArgs<Action>,
  ): Promise<InferResult<Action>>;
  async action(action: string | AnyApi, args?: NamedArgs): Promise<unknown>;
  async action<Action extends ConvexActionReference<any, any>>(
    action: Action | string | AnyApi,
    args?: InferArgs<Action> | NamedArgs,
  ): Promise<InferResult<Action> | unknown> {
    return super.action(
      coerceActionReference(action) as ConvexActionReference<any, any>,
      args as InferArgs<Action>,
    ) as Promise<
      InferResult<Action> | unknown
    >;
  }

  async paginatedQuery<Query extends ConvexPaginatedQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query> | undefined,
    pageSize: number,
    cursor: string | null,
  ): Promise<ConvexPage<InferResult<Query>>> {
    return super.paginatedQuery(query, args, pageSize, cursor) as Promise<
      ConvexPage<InferResult<Query>>
    >;
  }

  async scheduleAfter<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAfterMs: number,
  ): Promise<string>;
  async scheduleAfter(
    mutation: string | AnyApi,
    args: NamedArgs | undefined,
    runAfterMs: number,
  ): Promise<string>;
  async scheduleAfter<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args: InferArgs<Mutation> | NamedArgs | undefined,
    runAfterMs: number,
  ): Promise<string> {
    return super.scheduleAfter(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation> | undefined,
      runAfterMs,
    );
  }

  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAtMs: number,
  ): Promise<string>;
  async scheduleAt(
    mutation: string | AnyApi,
    args: NamedArgs | undefined,
    runAtMs: number,
  ): Promise<string>;
  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation | string | AnyApi,
    args: InferArgs<Mutation> | NamedArgs | undefined,
    runAtMs: number,
  ): Promise<string> {
    return super.scheduleAt(
      coerceMutationReference(mutation) as ConvexMutationReference<any, any>,
      args as InferArgs<Mutation> | undefined,
      runAtMs,
    );
  }

  onUpdate<Query extends ConvexQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query>,
    callback: (result: InferResult<Query>) => unknown,
    onError?: (error: Error) => unknown,
  ): Unsubscribe<InferResult<Query>>;
  onUpdate(
    query: string | AnyApi,
    args: NamedArgs,
    callback: (result: unknown) => unknown,
    onError?: (error: Error) => unknown,
  ): Unsubscribe<unknown>;
  onUpdate<Query extends ConvexQueryReference<any, any> | ConvexPaginatedQueryReference<any, any>>(
    query: Query | string | AnyApi,
    args: InferArgs<Query> | NamedArgs,
    callback: (
      result: Query extends ConvexPaginatedQueryReference<any, infer Item>
        ? Item[]
        : InferResult<Query>,
    ) => unknown,
    onError?: (error: Error) => unknown,
    options?: { pageSize?: number; cursor?: string | null },
  ): Unsubscribe<
    Query extends ConvexPaginatedQueryReference<any, infer Item>
      ? Item[]
      : InferResult<Query>
  > {
    if (typeof query === "string" || isAnyApiReference(query)) {
      assertNamedLiveQueryOptions(query, options);
      return super.onUpdate(
        coerceQueryReference(query) as ConvexQueryReference<any, any>,
        args as NamedArgs,
        callback as (result: unknown) => unknown,
        onError,
      ) as Unsubscribe<
        Query extends ConvexPaginatedQueryReference<any, infer Item>
          ? Item[]
          : InferResult<Query>
      >;
    }
    return super.onUpdate(query as Query, args as InferArgs<Query>, callback, onError, options) as Unsubscribe<
      Query extends ConvexPaginatedQueryReference<any, infer Item>
        ? Item[]
        : InferResult<Query>
    >;
  }
}
