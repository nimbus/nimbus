import {
  useCallback,
  createContext,
  createElement,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { Dispatch, ReactNode, SetStateAction } from "react";

import type {
  ActionReference,
  MutationReference,
  PaginatedQueryReference,
  QueryReference,
  InferArgs,
  InferResult,
} from "./internal/shared.ts";
import type { AuthTokenFetcher, ConnectionState } from "./browser.ts";
import { NimbusReactClient } from "./browser.ts";

export { NimbusReactClient } from "./browser.ts";
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
    query: QueryReference<any, any>;
    args?: Record<string, unknown>;
  }
>;

export type UseQueriesResults<Queries extends UseQueriesRequest> = {
  [Key in keyof Queries]: InferResult<Queries[Key]["query"]> | undefined | Error;
};

type QuerySnapshot<Result> = {
  requestKey: string;
  value: Result | undefined;
  error: Error | null;
};

type QueriesSnapshot<Queries extends UseQueriesRequest> = {
  requestKey: string;
  results: UseQueriesResults<Queries>;
};

type PaginatedSnapshot<Item> = {
  requestKey: string;
  results: Item[];
  nextCursor: string | null;
  status: PaginationStatus;
  requestedCount: number;
  refreshToken: number;
  error: Error | null;
};

const NimbusContext = createContext<NimbusReactClient | undefined>(undefined);
const NimbusAuthContext = createContext<{
  isLoading: boolean;
  isAuthenticated: boolean;
} | undefined>(undefined);

export function NimbusProvider(props: {
  client: NimbusReactClient;
  children?: ReactNode;
}) {
  return createElement(
    NimbusContext.Provider,
    { value: props.client },
    props.children,
  );
}

export type NimbusAuthState = {
  isLoading: boolean;
  isAuthenticated: boolean;
};

export function useNimbusAuth(): NimbusAuthState {
  const authState = useContext(NimbusAuthContext);
  if (authState === undefined) {
    throw new Error("Could not find a provider with auth as an ancestor component.");
  }
  return authState;
}

export function NimbusProviderWithAuth(props: {
  client: NimbusReactClient;
  children?: ReactNode;
  useAuth: () => {
    isLoading: boolean;
    isAuthenticated: boolean;
    fetchAccessToken: AuthTokenFetcher;
  };
}) {
  const {
    isLoading: authProviderLoading,
    isAuthenticated: authProviderAuthenticated,
    fetchAccessToken,
  } = props.useAuth();
  const [isClientAuthenticated, setIsClientAuthenticated] = useState<boolean | null>(null);

  if (authProviderLoading && isClientAuthenticated !== null) {
    setIsClientAuthenticated(null);
  }

  if (
    !authProviderLoading
    && !authProviderAuthenticated
    && isClientAuthenticated !== false
  ) {
    setIsClientAuthenticated(false);
  }

  return createElement(
    NimbusAuthContext.Provider,
    {
      value: {
        isLoading: isClientAuthenticated === null,
        isAuthenticated:
          authProviderAuthenticated && (isClientAuthenticated ?? false),
      },
    },
    createElement(NimbusAuthStateFirstEffect, {
      authProviderAuthenticated,
      fetchAccessToken,
      client: props.client,
      setIsClientAuthenticated,
    }),
    createElement(
      NimbusProvider,
      { client: props.client },
      props.children,
    ),
    createElement(NimbusAuthStateLastEffect, {
      authProviderAuthenticated,
      client: props.client,
      setIsClientAuthenticated,
    }),
  );
}

export function useNimbus() {
  const client = useContext(NimbusContext);
  if (client === undefined) {
    throw new Error("Could not find a client! This hook must be used under a provider.");
  }
  return client;
}

function NimbusAuthStateFirstEffect(props: {
  authProviderAuthenticated: boolean;
  fetchAccessToken: AuthTokenFetcher;
  client: NimbusReactClient;
  setIsClientAuthenticated: Dispatch<SetStateAction<boolean | null>>;
}) {
  useEffect(() => {
    let isRelevant = true;
    if (props.authProviderAuthenticated) {
      props.client.setAuth(props.fetchAccessToken, (isAuthenticated) => {
        if (isRelevant) {
          props.setIsClientAuthenticated(() => isAuthenticated);
        }
      });
      return () => {
        isRelevant = false;
        props.setIsClientAuthenticated((current) => (current ? false : null));
      };
    }
  }, [
    props.authProviderAuthenticated,
    props.client,
    props.fetchAccessToken,
    props.setIsClientAuthenticated,
  ]);
  return null;
}

function NimbusAuthStateLastEffect(props: {
  authProviderAuthenticated: boolean;
  client: NimbusReactClient;
  setIsClientAuthenticated: Dispatch<SetStateAction<boolean | null>>;
}) {
  useEffect(() => {
    if (props.authProviderAuthenticated) {
      return () => {
        props.client.clearAuth();
        props.setIsClientAuthenticated(() => null);
      };
    }
  }, [
    props.authProviderAuthenticated,
    props.client,
    props.setIsClientAuthenticated,
  ]);
  return null;
}

export function useNimbusConnectionState(): ConnectionState {
  const client = useNimbus();
  return useSyncExternalStore(
    (callback) => client.subscribeToConnectionState(callback),
    () => client.connectionState(),
    () => client.connectionState(),
  );
}

export function useQuery<Query extends QueryReference<any, any>>(
  query: Query,
  args?: InferArgs<Query> | "skip",
): InferResult<Query> | undefined {
  const client = useNimbus();
  const skip = args === "skip";
  const requestKey = buildRequestKey(query.kind, query.name, args);
  const [snapshot, setSnapshot] = useState<QuerySnapshot<InferResult<Query>>>(
    () => ({
      requestKey,
      value: undefined,
      error: null,
    }),
  );
  const activeSnapshot =
    !skip && snapshot.requestKey === requestKey
      ? snapshot
      : {
          requestKey,
          value: undefined,
          error: null,
        };

  useEffect(() => {
    if (skip) {
      return;
    }

    setSnapshot({
      requestKey,
      value: undefined,
      error: null,
    });

    const unsubscribe = client.onUpdate(
      query,
      (args ?? {}) as InferArgs<Query>,
      (next) => {
        setSnapshot((current) =>
          current.requestKey === requestKey
            ? {
                requestKey,
                value: next,
                error: null,
              }
            : current,
        );
      },
      (nextError) => {
        setSnapshot((current) =>
          current.requestKey === requestKey
            ? {
                requestKey,
                value: undefined,
                error: nextError,
              }
            : current,
        );
      },
    );
    return () => unsubscribe();
  }, [client, query, requestKey, skip]);

  if (activeSnapshot.error) {
    throw activeSnapshot.error;
  }

  return activeSnapshot.value;
}

export function useMutation<Mutation extends MutationReference<any, any>>(
  mutation: Mutation,
) {
  const client = useNimbus();
  const latestClient = useRef(client);
  const latestMutation = useRef(mutation);
  latestClient.current = client;
  latestMutation.current = mutation;

  return useCallback(async (args?: InferArgs<Mutation>) => {
    return latestClient.current.mutation(latestMutation.current, args);
  }, []);
}

export function useAction<Action extends ActionReference<any, any>>(
  action: Action,
) {
  const client = useNimbus();
  const latestClient = useRef(client);
  const latestAction = useRef(action);
  latestClient.current = client;
  latestAction.current = action;

  return useCallback(async (args?: InferArgs<Action>) => {
    return latestClient.current.action(latestAction.current, args);
  }, []);
}

export function useQueries<Queries extends UseQueriesRequest>(
  queries: Queries,
): UseQueriesResults<Queries> {
  const client = useNimbus();
  const rawQueryEntries = Object.entries(queries) as Array<
    [
      keyof Queries & string,
      {
        query: QueryReference<any, any>;
        args?: Record<string, unknown>;
      },
    ]
  >;
  const requestKey = JSON.stringify(
    rawQueryEntries.map(([key, value]) => [
      key,
      value.query.kind,
      value.query.name,
      value.args ?? {},
    ]),
  );
  const queryEntries = useMemo(() => rawQueryEntries, [requestKey]);
  const emptyResults = useMemo(
    () =>
      Object.fromEntries(
        queryEntries.map(([key]) => [key, undefined]),
      ) as UseQueriesResults<Queries>,
    [requestKey],
  );
  const [snapshot, setSnapshot] = useState<QueriesSnapshot<Queries>>(() => ({
    requestKey,
    results: emptyResults,
  }));
  const activeResults =
    snapshot.requestKey === requestKey ? snapshot.results : emptyResults;

  useEffect(() => {
    setSnapshot({
      requestKey,
      results: emptyResults,
    });

    const unsubscribes = queryEntries.map(([key, request]) =>
      client.onUpdate(
        request.query,
        (request.args ?? {}) as InferArgs<(typeof request)["query"]>,
        (value) => {
          setSnapshot((current) =>
            current.requestKey === requestKey
              ? {
                  requestKey,
                  results: {
                    ...current.results,
                    [key]: value,
                  },
                }
              : current,
          );
        },
        (error) => {
          setSnapshot((current) =>
            current.requestKey === requestKey
              ? {
                  requestKey,
                  results: {
                    ...current.results,
                    [key]: error,
                  },
                }
              : current,
          );
        },
      ),
    );

    return () => {
      for (const unsubscribe of unsubscribes) {
        unsubscribe();
      }
    };
  }, [client, emptyResults, queryEntries, requestKey]);

  return activeResults;
}

export function usePaginatedQuery<Query extends PaginatedQueryReference<any, any>>(
  query: Query,
  args: InferArgs<Query> | "skip",
  options: { initialNumItems: number },
): UsePaginatedQueryResult<InferResult<Query>> {
  const client = useNimbus();
  const skip = args === "skip";
  const requestKey = buildRequestKey(
    query.kind,
    query.name,
    args,
    options.initialNumItems,
  );
  const [snapshot, setSnapshot] = useState<PaginatedSnapshot<InferResult<Query>>>(
    () => createPaginatedSnapshot(requestKey, options.initialNumItems, skip),
  );
  const activeSnapshot: PaginatedSnapshot<InferResult<Query>> =
    snapshot.requestKey === requestKey
      ? snapshot
      : createPaginatedSnapshot(requestKey, options.initialNumItems, skip);

  useEffect(() => {
    setSnapshot(createPaginatedSnapshot(requestKey, options.initialNumItems, skip));
  }, [options.initialNumItems, requestKey, skip]);

  useEffect(() => {
    if (skip) {
      return;
    }

    const unsubscribe = client.onUpdate(
      query,
      (args ?? {}) as InferArgs<Query>,
      () => {
        setSnapshot((current) =>
          current.requestKey === requestKey
            ? {
                ...current,
                error: null,
                refreshToken: current.refreshToken + 1,
              }
            : current,
        );
      },
      (nextError) => {
        setSnapshot((current) =>
          current.requestKey === requestKey
            ? {
                ...current,
                error: nextError,
              }
            : current,
        );
      },
      {
        pageSize: activeSnapshot.requestedCount,
        cursor: null,
      },
    );

    return () => {
      unsubscribe();
    };
  }, [activeSnapshot.requestedCount, client, query, requestKey, skip]);

  useEffect(() => {
    let cancelled = false;
    if (skip) {
      return;
    }

    setSnapshot((current) => {
      if (current.requestKey !== requestKey) {
        return current;
      }
      return {
        ...current,
        error: null,
        status:
          current.status === "LoadingMore"
            ? current.status
            : current.results.length === 0
              ? "LoadingFirstPage"
              : current.status,
      };
    });

    void client
      .paginatedQuery(
        query,
        (args ?? {}) as InferArgs<Query>,
        activeSnapshot.requestedCount,
        null,
      )
      .then((page) => {
        if (cancelled) {
          return;
        }
        setSnapshot((current) =>
          current.requestKey === requestKey
            ? {
                ...current,
                error: null,
                results: page.data,
                nextCursor: page.next_cursor,
                status: page.has_more ? "CanLoadMore" : "Exhausted",
              }
            : current,
        );
      })
      .catch((nextError: unknown) => {
        if (cancelled) {
          return;
        }
        setSnapshot((current) =>
          current.requestKey === requestKey
            ? {
                ...current,
                error:
                  nextError instanceof Error
                    ? nextError
                    : new Error("convex paginated query failed"),
              }
            : current,
        );
      });

    return () => {
      cancelled = true;
    };
  }, [
    activeSnapshot.refreshToken,
    activeSnapshot.requestedCount,
    client,
    query,
    requestKey,
    skip,
  ]);

  if (activeSnapshot.error) {
    throw activeSnapshot.error;
  }

  return {
    results: activeSnapshot.results,
    status: activeSnapshot.status,
    isLoading:
      activeSnapshot.status === "LoadingFirstPage" ||
      activeSnapshot.status === "LoadingMore",
    loadMore: useCallback(
      (numItems: number) => {
        if (skip) {
          return;
        }

        setSnapshot((current) => {
          if (
            current.requestKey !== requestKey ||
            current.nextCursor === null ||
            current.status !== "CanLoadMore"
          ) {
            return current;
          }

          return {
            ...current,
            status: "LoadingMore",
            requestedCount: current.requestedCount + numItems,
          };
        });
      },
      [requestKey, skip],
    ),
  };
}

function buildRequestKey(
  kind: string,
  name: string,
  args: unknown,
  ...extras: unknown[]
) {
  return JSON.stringify([kind, name, args === "skip" ? "skip" : args ?? {}, ...extras]);
}

function createPaginatedSnapshot<Item>(
  requestKey: string,
  initialNumItems: number,
  skip: boolean,
): PaginatedSnapshot<Item> {
  return {
    requestKey,
    results: [],
    nextCursor: null,
    status: skip ? "Exhausted" : "LoadingFirstPage",
    requestedCount: initialNumItems,
    refreshToken: 0,
    error: null,
  };
}
