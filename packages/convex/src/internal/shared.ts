export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export type FunctionVisibility = "public" | "internal";

export type FilterOp = "eq" | "neq" | "gt" | "gte" | "lt" | "lte";
export type OrderDirection = "asc" | "desc";

export type QueryShape = {
  table: string;
  filters: Array<{
    field: string;
    op: FilterOp;
    value: JsonValue;
  }>;
  order: { field: string; direction: OrderDirection } | null;
  limit: number | null;
};

export type PaginatedQueryShape = {
  query: QueryShape;
  page_size: number;
  after: string | null;
};

export type MutationShape =
  | {
      type: "insert";
      table: string;
      fields: Record<string, JsonValue>;
    }
  | {
      type: "update";
      table: string;
      id: string;
      patch: Record<string, JsonValue>;
    }
  | {
      type: "delete";
      table: string;
      id: string;
    };

export type ActionShape =
  | { type: "query"; query: QueryShape }
  | { type: "paginated_query"; query: PaginatedQueryShape }
  | { type: "mutation"; mutation: MutationShape };

type ArgsMarker<Args> = { readonly _argsType?: Args };
type ResultMarker<Result> = { readonly _returnType?: Result };

export type ConvexQueryReference<Args, Result> = {
  kind: "query";
  name: string;
  visibility?: FunctionVisibility;
  resolve?: (args: Args) => QueryShape;
} & ArgsMarker<Args> &
  ResultMarker<Result>;

export type ConvexPaginatedQueryReference<Args, Item> = {
  kind: "paginated_query";
  name: string;
  visibility?: FunctionVisibility;
  resolve?: (args: Args) => QueryShape;
} & ArgsMarker<Args> &
  ResultMarker<Item>;

export type ConvexMutationReference<Args, Result> = {
  kind: "mutation";
  name: string;
  visibility?: FunctionVisibility;
  resolve?: (args: Args) => MutationShape;
} & ArgsMarker<Args> &
  ResultMarker<Result>;

export type ConvexActionReference<Args, Result> = {
  kind: "action";
  name: string;
  visibility?: FunctionVisibility;
  resolve?: (args: Args) => ActionShape;
} & ArgsMarker<Args> &
  ResultMarker<Result>;

export type ConvexFunctionReference<Args, Result> =
  | ConvexQueryReference<Args, Result>
  | ConvexPaginatedQueryReference<Args, Result>
  | ConvexMutationReference<Args, Result>
  | ConvexActionReference<Args, Result>;

export type InferArgs<Ref> = Ref extends ArgsMarker<infer Args> ? Args : never;
export type InferResult<Ref> = Ref extends ConvexPaginatedQueryReference<any, infer Item>
  ? Item
  : Ref extends ResultMarker<infer Result>
    ? Result
    : never;

export type ConvexPage<T> = {
  data: T[];
  next_cursor: string | null;
  has_more: boolean;
};

export function defineQuery<Args, Result>(
  name: string,
  resolve: (args: Args) => QueryShape,
): ConvexQueryReference<Args, Result> {
  return { kind: "query", name, visibility: "public", resolve };
}

export function definePaginatedQuery<Args, Item>(
  name: string,
  resolve: (args: Args) => QueryShape,
): ConvexPaginatedQueryReference<Args, Item> {
  return { kind: "paginated_query", name, visibility: "public", resolve };
}

export function defineMutation<Args, Result>(
  name: string,
  resolve: (args: Args) => MutationShape,
): ConvexMutationReference<Args, Result> {
  return { kind: "mutation", name, visibility: "public", resolve };
}

export function defineAction<Args, Result>(
  name: string,
  resolve: (args: Args) => ActionShape,
): ConvexActionReference<Args, Result> {
  return { kind: "action", name, visibility: "public", resolve };
}

export function makeQueryReference<
  Args extends Record<string, JsonValue> = Record<string, JsonValue>,
  Result = unknown,
>(
  name: string,
  visibility: FunctionVisibility = "public",
): ConvexQueryReference<Args, Result> {
  return { kind: "query", name, visibility };
}

export function makePaginatedQueryReference<
  Args extends Record<string, JsonValue> = Record<string, JsonValue>,
  Item = unknown,
>(
  name: string,
  visibility: FunctionVisibility = "public",
): ConvexPaginatedQueryReference<Args, Item> {
  return { kind: "paginated_query", name, visibility };
}

export function makeMutationReference<
  Args extends Record<string, JsonValue> = Record<string, JsonValue>,
  Result = unknown,
>(
  name: string,
  visibility: FunctionVisibility = "public",
): ConvexMutationReference<Args, Result> {
  return { kind: "mutation", name, visibility };
}

export function makeActionReference<
  Args extends Record<string, JsonValue> = Record<string, JsonValue>,
  Result = unknown,
>(
  name: string,
  visibility: FunctionVisibility = "public",
): ConvexActionReference<Args, Result> {
  return { kind: "action", name, visibility };
}

export function validateDeploymentUrl(address: string) {
  if (typeof address !== "string") {
    throw new Error(
      `Convex clients require a URL string, received ${typeof address}.`,
    );
  }
  if (!address.includes("://")) {
    throw new Error("Provided address was not an absolute URL.");
  }
}

export function stripTrailingSlash(url: string) {
  return url.endsWith("/") ? url.slice(0, -1) : url;
}

export function websocketUrlFromBase(baseUrl: string) {
  const url = new URL(stripTrailingSlash(baseUrl));
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = `${url.pathname}/ws`;
  url.search = "";
  url.hash = "";
  return url.toString();
}

export function normalizeArgs<Args>(args: Args | undefined): Args {
  return (args ?? ({} as Args)) as Args;
}

export function createConvexError(response: unknown, fallback: string) {
  if (typeof response === "string" && response.length > 0) {
    return new Error(response);
  }
  if (
    response &&
    typeof response === "object" &&
    "error" in response &&
    typeof response.error === "string"
  ) {
    return new Error(response.error);
  }
  return new Error(fallback);
}
