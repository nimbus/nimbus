import type {
  ActionReference,
  MutationReference,
  PaginatedQueryReference,
  QueryReference,
  InferResult,
  ActionShape,
  MutationShape,
  QueryShape,
} from "./internal/shared.ts";

export type LiveQueryReference<Args, Result> =
  | QueryReference<Args, Result>
  | PaginatedQueryReference<Args, Result>;

export type InferLiveResult<Ref> = Ref extends PaginatedQueryReference<any, infer Item>
  ? Item[]
  : InferResult<Ref>;

export type SubscriptionEntry<T> = {
  query: LiveQueryReference<any, any>;
  args: unknown;
  livePageSize?: number;
  liveCursor?: string | null;
  callback: (value: T) => unknown;
  onError?: (error: Error) => unknown;
  currentValue?: T;
  subscriptionId?: number;
  pendingRequestId?: string;
  unsubscribed: boolean;
};

export function decodeJwtPayload(token: string): Record<string, unknown> | null {
  const segments = token.split(".");
  if (segments.length !== 3) {
    return null;
  }
  try {
    return JSON.parse(decodeBase64UrlUtf8(segments[1])) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function decodeBase64UrlUtf8(segment: string): string {
  const padded = segment.replace(/-/g, "+").replace(/_/g, "/");
  const remainder = padded.length % 4;
  const base64 = remainder === 0 ? padded : `${padded}${"=".repeat(4 - remainder)}`;
  return decodeURIComponent(
    Array.from(globalThis.atob(base64), (char) =>
      `%${char.charCodeAt(0).toString(16).padStart(2, "0")}`,
    ).join(""),
  );
}

export function hasResolver<Args, Result>(
  reference:
    | QueryReference<Args, Result>
    | PaginatedQueryReference<Args, Result>
    | MutationReference<Args, Result>
    | ActionReference<Args, Result>,
): reference is
  | (QueryReference<Args, Result> & { resolve: (args: Args) => QueryShape })
  | (PaginatedQueryReference<Args, Result> & {
      resolve: (args: Args) => QueryShape;
    })
  | (MutationReference<Args, Result> & {
      resolve: (args: Args) => MutationShape;
    })
  | (ActionReference<Args, Result> & {
      resolve: (args: Args) => ActionShape;
    }) {
  return typeof reference.resolve === "function";
}

export function attachSocketListener(
  socket: { addEventListener?: (type: string, listener: (event: any) => void) => void; on?: (type: string, listener: (event: any) => void) => void; },
  type: string,
  listener: (event: any) => void,
) {
  if (typeof socket.addEventListener === "function") {
    socket.addEventListener(type, listener);
    return;
  }
  if (typeof socket.on === "function") {
    socket.on(type, (event) => {
      if (type === "message") {
        const payload =
          event && typeof event === "object" && "data" in event
            ? (event as { data: unknown }).data
            : event;
        listener({ data: typeof payload === "string" ? payload : String(payload) });
        return;
      }
      listener(event);
    });
    return;
  }
  throw new Error(`Configured WebSocket implementation does not support "${type}" listeners.`);
}

export function buildSubscribeMessage<Args, Result>(
  query: LiveQueryReference<Args, Result>,
  requestId: string,
  args: Args,
  options?: { pageSize?: number; cursor?: string | null },
) {
  if (hasResolver(query)) {
    return {
      type: "subscribe",
      request_id: requestId,
      query: query.resolve(args),
    };
  }

  return {
    type: "subscribe_named",
    request_id: requestId,
    name: query.name,
    args,
    ...(query.kind === "paginated_query" && typeof options?.pageSize === "number"
      ? {
          page_size: options.pageSize,
          cursor: options.cursor ?? null,
        }
      : {}),
  };
}

export function areSubscriptionValuesEqual(previous: unknown, next: unknown) {
  if (previous === undefined) {
    return false;
  }

  return JSON.stringify(previous) === JSON.stringify(next);
}
