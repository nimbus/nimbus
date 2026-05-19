export type LoadingValue<T> =
  | { kind: "loading" }
  | { kind: "ok"; value: T }
  | { kind: "offline" }
  | { kind: "error"; message: string };

export type ConnectionSnapshot = {
  isWebSocketConnected: boolean;
  hasEverConnected: boolean;
};

export function toLoadingValue<T>(
  value: T | null | undefined,
  conn: ConnectionSnapshot,
): LoadingValue<NonNullable<T>> {
  if (value !== undefined && value !== null) {
    return { kind: "ok", value: value as NonNullable<T> };
  }
  if (conn.isWebSocketConnected) {
    return { kind: "loading" };
  }
  if (conn.hasEverConnected) {
    return { kind: "offline" };
  }
  return { kind: "loading" };
}

export function isLoading(v: LoadingValue<unknown>): boolean {
  return v.kind === "loading";
}

export function isOffline(v: LoadingValue<unknown>): boolean {
  return v.kind === "offline";
}
