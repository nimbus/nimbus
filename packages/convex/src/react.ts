import { createElement } from "react";
import type { ReactNode } from "react";

import {
  NeovexProvider,
  NeovexProviderWithAuth,
  useAction as useNeovexAction,
  useMutation as useNeovexMutation,
  useNeovex,
  useNeovexAuth,
  useNeovexConnectionState,
  usePaginatedQuery as useNeovexPaginatedQuery,
  useQueries as useNeovexQueries,
  useQuery as useNeovexQuery,
} from "neovex/react";

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
    NeovexProvider,
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
    NeovexProviderWithAuth,
    {
      client: props.client,
      useAuth: props.useAuth,
    },
    props.children,
  );
}

export function useConvex(): ConvexReactClient {
  return useNeovex() as ConvexReactClient;
}

export function useConvexAuth(): ConvexAuthState {
  return useNeovexAuth();
}

export function useConvexConnectionState(): ConnectionState {
  return useNeovexConnectionState();
}

export function useQuery<Query extends ConvexQueryReference<any, any>>(
  query: Query,
  args?: InferArgs<Query> | "skip",
): InferResult<Query> | undefined {
  return useNeovexQuery(query, args) as InferResult<Query> | undefined;
}

export function useMutation<Mutation extends ConvexMutationReference<any, any>>(
  mutation: Mutation,
) {
  return useNeovexMutation(mutation) as (
    args?: InferArgs<Mutation>,
  ) => Promise<InferResult<Mutation>>;
}

export function useAction<Action extends ConvexActionReference<any, any>>(
  action: Action,
) {
  return useNeovexAction(action) as (
    args?: InferArgs<Action>,
  ) => Promise<InferResult<Action>>;
}

export function useQueries<Queries extends UseQueriesRequest>(
  queries: Queries,
): UseQueriesResults<Queries> {
  return useNeovexQueries(queries) as UseQueriesResults<Queries>;
}

export function usePaginatedQuery<Query extends ConvexPaginatedQueryReference<any, any>>(
  query: Query,
  args: InferArgs<Query> | "skip",
  options: { initialNumItems: number },
): UsePaginatedQueryResult<InferResult<Query>> {
  return useNeovexPaginatedQuery(
    query,
    args,
    options,
  ) as UsePaginatedQueryResult<InferResult<Query>>;
}
