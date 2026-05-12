import { createElement } from "react";
import type { ReactNode } from "react";

import {
  NimbusProvider,
  NimbusProviderWithAuth,
  useAction as useNimbusAction,
  useMutation as useNimbusMutation,
  useNimbus,
  useNimbusAuth,
  useNimbusConnectionState,
  usePaginatedQuery as useNimbusPaginatedQuery,
  useQueries as useNimbusQueries,
  useQuery as useNimbusQuery,
} from "nimbus/react";

import { ConvexReactClient } from "./browser.ts";
import type {
  AuthTokenFetcher,
  ConnectionState,
} from "./browser.ts";
import type {
  ConvexActionReference,
  ConvexMutationReference,
  ConvexPaginatedQueryReference,
  ConvexQueryReference,
  InferArgs,
  InferResult,
} from "./internal/shared.ts";

export { ConvexReactClient } from "./browser.ts";
export type { ConnectionState } from "./browser.ts";

export type PaginationStatus =
  | "LoadingFirstPage"
  | "CanLoadMore"
  | "LoadingMore"
  | "Exhausted";

export type UsePaginatedQueryResult<T> = {
  results: T[];
  status: PaginationStatus;
  isLoading: boolean;
  loadMore: (numItems: number) => void;
};

export type UseQueriesRequest = Record<
  string,
  {
    query: ConvexQueryReference<any, any>;
    args?: Record<string, unknown>;
  }
>;

export type UseQueriesResults<Queries extends UseQueriesRequest> = {
  [Key in keyof Queries]: InferResult<Queries[Key]["query"]> | undefined | Error;
};

export type ConvexAuthState = {
  isLoading: boolean;
  isAuthenticated: boolean;
};

export function ConvexProvider(props: {
  client: ConvexReactClient;
  children?: ReactNode;
}) {
  return createElement(
    NimbusProvider,
    { client: props.client },
    props.children,
  );
}

export function ConvexProviderWithAuth(props: {
  client: ConvexReactClient;
  children?: ReactNode;
  useAuth: () => {
    isLoading: boolean;
    isAuthenticated: boolean;
    fetchAccessToken: AuthTokenFetcher;
  };
}) {
  return createElement(
    NimbusProviderWithAuth,
    {
      client: props.client,
      useAuth: props.useAuth,
    },
    props.children,
  );
}

export function useConvex(): ConvexReactClient {
  return useNimbus() as ConvexReactClient;
}

export function useConvexAuth(): ConvexAuthState {
  return useNimbusAuth();
}

export function useConvexConnectionState(): ConnectionState {
  return useNimbusConnectionState();
}

export function useQuery<Query extends ConvexQueryReference<any, any>>(
  query: Query,
  args?: InferArgs<Query> | "skip",
): InferResult<Query> | undefined {
  return useNimbusQuery(query, args) as InferResult<Query> | undefined;
}

export function useMutation<Mutation extends ConvexMutationReference<any, any>>(
  mutation: Mutation,
) {
  return useNimbusMutation(mutation) as (
    args?: InferArgs<Mutation>,
  ) => Promise<InferResult<Mutation>>;
}

export function useAction<Action extends ConvexActionReference<any, any>>(
  action: Action,
) {
  return useNimbusAction(action) as (
    args?: InferArgs<Action>,
  ) => Promise<InferResult<Action>>;
}

export function useQueries<Queries extends UseQueriesRequest>(
  queries: Queries,
): UseQueriesResults<Queries> {
  return useNimbusQueries(queries) as UseQueriesResults<Queries>;
}

export function usePaginatedQuery<Query extends ConvexPaginatedQueryReference<any, any>>(
  query: Query,
  args: InferArgs<Query> | "skip",
  options: { initialNumItems: number },
): UsePaginatedQueryResult<InferResult<Query>> {
  return useNimbusPaginatedQuery(
    query,
    args,
    options,
  ) as UsePaginatedQueryResult<InferResult<Query>>;
}
