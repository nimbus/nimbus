---
title: SDK React bindings
description: Reference for @nimbus/nimbus/react — providers and hooks for queries, mutations, actions, pagination, auth, and connection state.
sidebar:
  label: React
  order: 5
---

The `@nimbus/nimbus/react` entry point provides a context provider and hooks
over `NimbusReactClient`. It requires the optional peer dependencies `react`
and `react-dom` (`^18` or `^19`).

## Setup

```tsx
import { NimbusProvider, NimbusReactClient } from "@nimbus/nimbus/react";

const client = new NimbusReactClient("http://127.0.0.1:8080/convex/demo");

export function App({ children }: { children: React.ReactNode }) {
  return <NimbusProvider client={client}>{children}</NimbusProvider>;
}
```

`NimbusProvider` takes `{ client: NimbusReactClient, children? }` and makes
the client available to all hooks below. `NimbusReactClient` is re-exported
from this entry point and has the same surface as `NimbusClient` — see
[Clients](/reference/sdk/client/).

## Hooks

### useQuery

```typescript
useQuery(query, args?): Result | undefined
```

Subscribes to a query and re-renders on every change. Returns `undefined`
until the first result arrives. Pass the string `"skip"` as `args` to skip
subscribing. If the subscription reports an error, the hook throws it during
render (pair with an error boundary).

```tsx
import { useQuery } from "@nimbus/nimbus/react";
import { makeQueryReference } from "@nimbus/nimbus/browser";

const listMessages = makeQueryReference<{ channel: string }>("messages:list");

function Messages({ channel }: { channel: string }) {
  const messages = useQuery(listMessages, { channel });
  if (messages === undefined) return <p>Loading…</p>;
  return <ul>{/* render messages */}</ul>;
}
```

### useMutation / useAction

```typescript
useMutation(mutation): (args?) => Promise<Result>
useAction(action): (args?) => Promise<Result>
```

Return a stable callback (identity does not change across renders) that runs
the mutation or action on the provider's client.

```tsx
import { useMutation } from "@nimbus/nimbus/react";
import { makeMutationReference } from "@nimbus/nimbus/browser";

const sendMessage = makeMutationReference<{ channel: string; body: string }>(
  "messages:send",
);

function Composer({ channel }: { channel: string }) {
  const send = useMutation(sendMessage);
  return (
    <button onClick={() => void send({ channel, body: "hello" })}>Send</button>
  );
}
```

### useQueries

```typescript
useQueries(queries: UseQueriesRequest): UseQueriesResults
```

Subscribes to a record of queries at once. `UseQueriesRequest` maps keys to
`{ query, args? }`. The result maps each key to the query's latest value,
`undefined` while loading, or an `Error` if that subscription failed
(per-key errors are returned, not thrown).

### usePaginatedQuery

```typescript
usePaginatedQuery(query, args | "skip", { initialNumItems }): UsePaginatedQueryResult
```

Subscribes to a paginated query reference and accumulates results.

```typescript
type UsePaginatedQueryResult<T> = {
  results: T[];
  status: "LoadingFirstPage" | "CanLoadMore" | "LoadingMore" | "Exhausted";
  isLoading: boolean; // status is LoadingFirstPage or LoadingMore
  loadMore: (numItems: number) => void;
};
```

`loadMore(numItems)` grows the requested result set by `numItems`; it is a
no-op unless `status` is `"CanLoadMore"`. Errors are thrown during render.
The `PaginationStatus` exported here is this hook-status union — distinct
from the server entry point's page-status type of the same name.

### useNimbus

```typescript
useNimbus(): NimbusReactClient
```

Returns the provider's client. Throws if no provider is an ancestor.

### useNimbusConnectionState

```typescript
useNimbusConnectionState(): ConnectionState
```

Subscribes to the client's connection state via `useSyncExternalStore`. See
[Clients](/reference/sdk/client/) for the `ConnectionState` fields.

## Auth integration

### NimbusProviderWithAuth

```tsx
<NimbusProviderWithAuth client={client} useAuth={useAuthFromMyProvider}>
  {children}
</NimbusProviderWithAuth>
```

`useAuth` is a hook you supply (typically wrapping your identity provider's
SDK) that returns:

```typescript
{
  isLoading: boolean;
  isAuthenticated: boolean;
  fetchAccessToken: AuthTokenFetcher; // ({ forceRefreshToken }) => Promise<string | null | undefined>
}
```

When the auth provider reports authenticated, the component calls
`client.setAuth(fetchAccessToken, …)` and tracks whether the *server*
accepted the token; when the provider logs out, it clears client auth. See
[Authentication](/developers/auth/) for the end-to-end flow.

### useNimbusAuth

```typescript
useNimbusAuth(): NimbusAuthState // { isLoading: boolean; isAuthenticated: boolean }
```

Reads the combined auth state under a `NimbusProviderWithAuth`:
`isAuthenticated` is `true` only after the server has confirmed the token,
and `isLoading` is `true` while that confirmation is pending. Throws if no
auth-aware provider is an ancestor.

## Exports — `@nimbus/nimbus/react`

Components and hooks: `NimbusProvider`, `NimbusProviderWithAuth`, `useQuery`,
`useMutation`, `useAction`, `useQueries`, `usePaginatedQuery`, `useNimbus`,
`useNimbusAuth`, `useNimbusConnectionState` — documented above.

Class: `NimbusReactClient` (re-export from the browser entry).

Types:

| Type | Description |
| --- | --- |
| `ConnectionState` | Re-export from the browser entry. |
| `NimbusAuthState` | `{ isLoading, isAuthenticated }`. |
| `PaginationStatus` | Hook loading-status union (see `usePaginatedQuery`). |
| `UsePaginatedQueryResult` | Return shape of `usePaginatedQuery`. |
| `UseQueriesRequest` | Input record for `useQueries`. |
| `UseQueriesResults` | Output record for `useQueries`. |
