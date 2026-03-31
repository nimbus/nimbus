import {
  NeovexClient,
  NeovexHttpClient,
  NeovexReactClient,
} from "neovex/browser";
import type {
  ConvexActionReference,
  ConvexMutationReference,
  ConvexPage,
  ConvexPaginatedQueryReference,
  ConvexQueryReference,
  InferArgs,
  InferResult,
} from "./internal/shared";

export {
  defineAction,
  defineMutation,
  definePaginatedQuery,
  defineQuery,
  makeActionReference,
  makeMutationReference,
  makePaginatedQueryReference,
  makeQueryReference,
} from "./internal/shared";

type FetchLike = typeof globalThis.fetch;

export type ConnectionState = {
  hasInflightRequests: boolean;
  isWebSocketConnected: boolean;
  timeOfOldestInflightRequest: Date | null;
  hasEverConnected: boolean;
  connectionCount: number;
  connectionRetries: number;
  inflightMutations: number;
  inflightActions: number;
};

export type Unsubscribe<T> = {
  (): void;
  unsubscribe(): void;
  getCurrentValue(): T | undefined;
  getQueryLogs(): string[] | undefined;
};

export type AuthTokenFetcher = (args: {
  forceRefreshToken: boolean;
}) => Promise<string | null | undefined>;

type ConvexHttpClientOptions = {
  skipConvexDeploymentUrlCheck?: boolean;
  auth?: string;
  fetch?: FetchLike;
};

type ConvexClientOptions = ConvexHttpClientOptions & {
  disabled?: boolean;
  authRefreshTokenLeewaySeconds?: number;
};

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

export class ConvexHttpClient extends NeovexHttpClient {
  constructor(address: string, options: ConvexHttpClientOptions = {}) {
    super(address, withConvexDeploymentUrlCheck(options));
  }

  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query,
    args?: InferArgs<Query>,
  ): Promise<InferResult<Query>> {
    return super.query(query, args) as Promise<InferResult<Query>>;
  }

  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args?: InferArgs<Mutation>,
  ): Promise<InferResult<Mutation>> {
    return super.mutation(mutation, args) as Promise<InferResult<Mutation>>;
  }

  async action<Action extends ConvexActionReference<any, any>>(
    action: Action,
    args?: InferArgs<Action>,
  ): Promise<InferResult<Action>> {
    return super.action(action, args) as Promise<InferResult<Action>>;
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
  ): Promise<string> {
    return super.scheduleAfter(mutation, args, runAfterMs);
  }

  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAtMs: number,
  ): Promise<string> {
    return super.scheduleAt(mutation, args, runAtMs);
  }
}

export class ConvexClient extends NeovexClient {
  constructor(address: string, options: ConvexClientOptions = {}) {
    super(address, withConvexDeploymentUrlCheck(options));
  }

  connectionState(): ConnectionState {
    return super.connectionState();
  }

  subscribeToConnectionState(callback: () => void) {
    return super.subscribeToConnectionState(callback);
  }

  setAuth(value: string | AuthTokenFetcher, onChange?: (isAuthenticated: boolean) => void) {
    super.setAuth(value, onChange);
  }

  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query,
    args?: InferArgs<Query>,
  ): Promise<InferResult<Query>> {
    return super.query(query, args) as Promise<InferResult<Query>>;
  }

  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args?: InferArgs<Mutation>,
  ): Promise<InferResult<Mutation>> {
    return super.mutation(mutation, args) as Promise<InferResult<Mutation>>;
  }

  async action<Action extends ConvexActionReference<any, any>>(
    action: Action,
    args?: InferArgs<Action>,
  ): Promise<InferResult<Action>> {
    return super.action(action, args) as Promise<InferResult<Action>>;
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
  ): Promise<string> {
    return super.scheduleAfter(mutation, args, runAfterMs);
  }

  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAtMs: number,
  ): Promise<string> {
    return super.scheduleAt(mutation, args, runAtMs);
  }

  onUpdate<Query extends ConvexQueryReference<any, any> | ConvexPaginatedQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query>,
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
    return super.onUpdate(query, args, callback, onError, options) as Unsubscribe<
      Query extends ConvexPaginatedQueryReference<any, infer Item>
        ? Item[]
        : InferResult<Query>
    >;
  }
}

export class ConvexReactClient extends NeovexReactClient {
  constructor(address: string, options: ConvexClientOptions = {}) {
    super(address, withConvexDeploymentUrlCheck(options));
  }

  connectionState(): ConnectionState {
    return super.connectionState();
  }

  subscribeToConnectionState(callback: () => void) {
    return super.subscribeToConnectionState(callback);
  }

  setAuth(value: string | AuthTokenFetcher, onChange?: (isAuthenticated: boolean) => void) {
    super.setAuth(value, onChange);
  }

  async query<Query extends ConvexQueryReference<any, any>>(
    query: Query,
    args?: InferArgs<Query>,
  ): Promise<InferResult<Query>> {
    return super.query(query, args) as Promise<InferResult<Query>>;
  }

  async mutation<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args?: InferArgs<Mutation>,
  ): Promise<InferResult<Mutation>> {
    return super.mutation(mutation, args) as Promise<InferResult<Mutation>>;
  }

  async action<Action extends ConvexActionReference<any, any>>(
    action: Action,
    args?: InferArgs<Action>,
  ): Promise<InferResult<Action>> {
    return super.action(action, args) as Promise<InferResult<Action>>;
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
  ): Promise<string> {
    return super.scheduleAfter(mutation, args, runAfterMs);
  }

  async scheduleAt<Mutation extends ConvexMutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAtMs: number,
  ): Promise<string> {
    return super.scheduleAt(mutation, args, runAtMs);
  }

  onUpdate<Query extends ConvexQueryReference<any, any> | ConvexPaginatedQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query>,
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
    return super.onUpdate(query, args, callback, onError, options) as Unsubscribe<
      Query extends ConvexPaginatedQueryReference<any, infer Item>
        ? Item[]
        : InferResult<Query>
    >;
  }
}
