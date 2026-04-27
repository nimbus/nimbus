import { FirestoreError } from "../firestore";
import type { ListenRequest } from "../gen/google/firestore/v1/firestore_pb";
import { fromBinary, toBinary, toJson, firestoreV1 } from "./protobuf";

const RETRYABLE_LISTEN_CLOSE_CODES = new Set([1006, 1011]);
const LISTEN_RECONNECT_DELAYS_MS = [0, 50, 250] as const;

export interface FirestoreWebSocketLike {
  binaryType?: string;
  addEventListener?: (type: string, listener: (event: unknown) => void) => void;
  on?: (type: string, listener: (event: unknown) => void) => void;
  send(data: ArrayBufferLike | ArrayBufferView): void;
  close(code?: number, reason?: string): void;
}

export type FirestoreWebSocketFactory = (
  url: string,
  protocols?: string | readonly string[],
) => FirestoreWebSocketLike;

export interface FirestoreListenWebSocketSession {
  close(): void;
}

export interface FirestoreListenReadTime {
  readonly seconds: bigint;
  readonly nanos: number;
}

export interface FirestoreListenResumeCursor {
  readonly resumeToken?: Uint8Array;
  readonly readTime?: FirestoreListenReadTime;
}

interface MutableFirestoreListenResumeCursor {
  resumeToken?: Uint8Array;
  readTime?: FirestoreListenReadTime;
}

export interface OpenFirestoreListenWebSocketOptions {
  readonly socketFactory?: FirestoreWebSocketFactory;
  readonly url: string;
  readonly targetId: number;
  readonly canRefreshAuthToken?: boolean;
  readonly buildAddTargetRequest: (
    cursor: FirestoreListenResumeCursor | null,
  ) => ListenRequest;
  readonly removeTargetRequest: ListenRequest;
  readonly resolveSubprotocols?: (
    forceRefresh: boolean,
  ) => Promise<readonly string[]>;
  readonly onResponse: (response: unknown) => void;
  readonly onError: (error: FirestoreError) => void;
  readonly onReconnect?: () => void;
}

export function openFirestoreListenWebSocket(
  options: OpenFirestoreListenWebSocketOptions,
): FirestoreListenWebSocketSession {
  let closed = false;
  let reconnectScheduled = false;
  let openAttempt = 0;
  let retainedCursor: MutableFirestoreListenResumeCursor | null = null;
  let currentSocket: FirestoreWebSocketLike | null = null;
  let currentSocketOpened = false;
  let currentSocketRemoved = false;
  let authRefreshConsumed = false;
  let reconnectAttempt = 0;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  void openSocket(false);

  return {
    close() {
      if (closed) {
        return;
      }
      closed = true;
      reconnectScheduled = false;
      reconnectAttempt = 0;
      if (reconnectTimer !== null) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
      if (currentSocketOpened && !currentSocketRemoved && currentSocket) {
        currentSocketRemoved = true;
        try {
          currentSocket.send(
            toBinary(firestoreV1.ListenRequestSchema, options.removeTargetRequest),
          );
        } catch {
          // Closing the socket is still the cleanup fallback.
        }
      }
      if (currentSocket) {
        safeClose(currentSocket);
      }
    },
  };

  async function openSocket(forceRefreshAuth: boolean): Promise<void> {
    const attempt = openAttempt + 1;
    openAttempt = attempt;
    let protocols: readonly string[] = [];
    try {
      protocols = (await options.resolveSubprotocols?.(forceRefreshAuth)) ?? [];
    } catch (error) {
      closed = true;
      fail(
        options.onError,
        normalizeListenError(
          error,
          forceRefreshAuth
            ? "Failed to refresh Firestore Listen auth token."
            : "Failed to resolve Firestore Listen auth token.",
          "UNAUTHENTICATED",
          401,
        ),
      );
      return;
    }
    if (closed || attempt !== openAttempt) {
      return;
    }

    const socket = createSocket(options.url, protocols, options.socketFactory);
    let expectedClose = false;
    currentSocket = socket;
    currentSocketOpened = false;
    currentSocketRemoved = false;

    if ("binaryType" in socket) {
      socket.binaryType = "arraybuffer";
    }

    attachSocketListener(socket, "open", () => {
      if (closed || currentSocket !== socket) {
        return;
      }
      currentSocketOpened = true;
      try {
        socket.send(
          toBinary(
            firestoreV1.ListenRequestSchema,
            options.buildAddTargetRequest(cloneResumeCursor(retainedCursor)),
          ),
        );
      } catch (error) {
        expectedClose = true;
        fail(
          options.onError,
          normalizeListenError(
            error,
            "Failed to send Firestore Listen addTarget frame.",
          ),
        );
        closed = true;
        safeClose(socket);
      }
    });

    attachSocketListener(socket, "message", (event) => {
      if (closed || currentSocket !== socket) {
        return;
      }
      try {
        const bytes = extractBinaryFrame(event);
        const response = fromBinary(firestoreV1.ListenResponseSchema, bytes);
        reconnectAttempt = 0;
        authRefreshConsumed = false;
        retainedCursor = updateRetainedCursor(
          retainedCursor,
          response,
          options.targetId,
        );
        options.onResponse(toJson(firestoreV1.ListenResponseSchema, response));
      } catch (error) {
        expectedClose = true;
        fail(
          options.onError,
          normalizeListenError(
            error,
            "Firestore Listen received an invalid binary protobuf frame.",
            "INVALID_ARGUMENT",
            400,
          ),
        );
        closed = true;
        safeClose(socket);
      }
    });

    attachSocketListener(socket, "error", () => {
      if (closed || expectedClose || currentSocket !== socket) {
        return;
      }
      expectedClose = true;
      scheduleReconnect(
        new FirestoreError("UNAVAILABLE", "Firestore Listen WebSocket failed.", 503),
        false,
      );
      safeClose(socket);
    });

    attachSocketListener(socket, "close", (event) => {
      if (closed || currentSocket !== socket) {
        return;
      }
      currentSocketOpened = false;
      if (expectedClose) {
        return;
      }
      const closeEvent = asCloseEvent(event);
      const closeError = closeEventToError(closeEvent);
      if (
        closeError.code === "UNAUTHENTICATED" &&
        options.canRefreshAuthToken &&
        !authRefreshConsumed
      ) {
        authRefreshConsumed = true;
        scheduleReconnect(closeError, true);
        return;
      }
      if (!RETRYABLE_LISTEN_CLOSE_CODES.has(closeEvent.code)) {
        closed = true;
        fail(options.onError, closeError);
        return;
      }
      scheduleReconnect(closeError, false);
    });
  }

  function scheduleReconnect(
    error: FirestoreError,
    forceRefreshAuth: boolean,
  ): void {
    if (closed || reconnectScheduled) {
      return;
    }
    let delayMs = 0;
    if (!forceRefreshAuth) {
      delayMs = LISTEN_RECONNECT_DELAYS_MS[reconnectAttempt] ?? -1;
      if (delayMs < 0) {
        closed = true;
        fail(options.onError, error);
        return;
      }
      reconnectAttempt += 1;
    }
    reconnectScheduled = true;
    if (typeof options.onReconnect === "function") {
      try {
        options.onReconnect();
      } catch {
        // Observer state resets stay user-owned; do not fail the transport.
      }
    }
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      if (closed) {
        return;
      }
      reconnectScheduled = false;
      void openSocket(forceRefreshAuth);
    }, delayMs);
  }
}

function createSocket(
  url: string,
  protocols: readonly string[],
  socketFactory?: FirestoreWebSocketFactory,
): FirestoreWebSocketLike {
  if (socketFactory) {
    return socketFactory(url, protocols.length > 0 ? protocols : undefined);
  }
  const SocketImpl = globalThis.WebSocket as
    | (new (url: string, protocols?: string | string[]) => FirestoreWebSocketLike)
    | undefined;
  if (typeof SocketImpl !== "function") {
    throw new Error("No WebSocket implementation is available for Firestore Listen.");
  }
  if (protocols.length === 0) {
    return new SocketImpl(url);
  }
  return new SocketImpl(
    url,
    protocols.length === 1 ? protocols[0] : Array.from(protocols),
  );
}

function attachSocketListener(
  socket: FirestoreWebSocketLike,
  type: string,
  listener: (event: unknown) => void,
): void {
  if (typeof socket.addEventListener === "function") {
    socket.addEventListener(type, listener);
    return;
  }
  if (typeof socket.on === "function") {
    socket.on(type, listener);
    return;
  }
  throw new Error(`Configured Firestore WebSocket does not support "${type}" listeners.`);
}

function extractBinaryFrame(event: unknown): Uint8Array {
  const payload =
    event && typeof event === "object" && "data" in event
      ? (event as { data: unknown }).data
      : event;
  if (payload instanceof Uint8Array) {
    return payload;
  }
  if (payload instanceof ArrayBuffer) {
    return new Uint8Array(payload);
  }
  if (ArrayBuffer.isView(payload)) {
    return new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength);
  }
  throw new Error("Firestore Listen WebSocket frames must be binary protobuf messages.");
}

function asCloseEvent(event: unknown): { code: number; reason: string } {
  if (event && typeof event === "object") {
    return {
      code:
        typeof (event as { code?: unknown }).code === "number"
          ? (event as { code: number }).code
          : 1006,
      reason:
        typeof (event as { reason?: unknown }).reason === "string"
          ? (event as { reason: string }).reason
          : "",
    };
  }
  return { code: 1006, reason: "" };
}

function closeEventToError(closeEvent: { code: number; reason: string }): FirestoreError {
  if (closeEvent.code === 1003) {
    return new FirestoreError(
      "INVALID_ARGUMENT",
      closeEvent.reason || "Firestore Listen WebSocket requires binary protobuf frames.",
      400,
    );
  }
  if (closeEvent.code === 1008) {
    const reason = closeEvent.reason || "Firestore Listen WebSocket rejected the request.";
    const normalizedReason = reason.toLowerCase();
    if (normalizedReason.includes("unauthenticated")) {
      return new FirestoreError("UNAUTHENTICATED", reason, 401);
    }
    if (normalizedReason.includes("permission")) {
      return new FirestoreError("PERMISSION_DENIED", reason, 403);
    }
    if (normalizedReason.includes("aborted")) {
      return new FirestoreError("ABORTED", reason, 409);
    }
    return new FirestoreError("FAILED_PRECONDITION", reason, 400);
  }
  return new FirestoreError(
    "UNAVAILABLE",
    closeEvent.reason || `Firestore Listen WebSocket closed (${closeEvent.code}).`,
    503,
  );
}

function cloneResumeCursor(
  cursor: MutableFirestoreListenResumeCursor | null,
): FirestoreListenResumeCursor | null {
  if (!cursor) {
    return null;
  }
  return {
    resumeToken: cursor.resumeToken ? new Uint8Array(cursor.resumeToken) : undefined,
    readTime: cursor.readTime
      ? {
          seconds: cursor.readTime.seconds,
          nanos: cursor.readTime.nanos,
        }
      : undefined,
  };
}

function updateRetainedCursor(
  cursor: MutableFirestoreListenResumeCursor | null,
  response: unknown,
  targetId: number,
): MutableFirestoreListenResumeCursor | null {
  if (
    !response ||
    typeof response !== "object" ||
    !("responseType" in response) ||
    typeof response.responseType !== "object" ||
    response.responseType === null ||
    !("case" in response.responseType) ||
    response.responseType.case !== "targetChange"
  ) {
    return cursor;
  }
  const targetChange =
    "value" in response.responseType ? response.responseType.value : null;
  if (!targetChange || typeof targetChange !== "object") {
    return cursor;
  }
  const targetIds = Array.isArray((targetChange as { targetIds?: unknown }).targetIds)
    ? ((targetChange as { targetIds: unknown[] }).targetIds.filter(
        (value): value is number => typeof value === "number",
      ) as number[])
    : [];
  if (targetIds.length > 0 && !targetIds.includes(targetId)) {
    return cursor;
  }

  const nextCursor: MutableFirestoreListenResumeCursor =
    cloneResumeCursor(cursor) ?? {};
  const resumeToken = (targetChange as { resumeToken?: unknown }).resumeToken;
  if (resumeToken instanceof Uint8Array && resumeToken.byteLength > 0) {
    nextCursor.resumeToken = new Uint8Array(resumeToken);
  }
  const readTime = (targetChange as { readTime?: unknown }).readTime;
  if (
    readTime &&
    typeof readTime === "object" &&
    "seconds" in readTime &&
    typeof readTime.seconds === "bigint" &&
    "nanos" in readTime &&
    typeof readTime.nanos === "number"
  ) {
    nextCursor.readTime = {
      seconds: readTime.seconds,
      nanos: readTime.nanos,
    };
  }
  return nextCursor;
}

function safeClose(socket: FirestoreWebSocketLike): void {
  try {
    socket.close();
  } catch {
    // Ignore close failures during shutdown.
  }
}

function normalizeListenError(
  error: unknown,
  fallbackMessage: string,
  code = "UNKNOWN",
  status = 500,
): FirestoreError {
  if (error instanceof FirestoreError) {
    return error;
  }
  if (error instanceof Error) {
    return new FirestoreError(code, error.message || fallbackMessage, status);
  }
  return new FirestoreError(code, fallbackMessage, status);
}

function fail(onError: (error: FirestoreError) => void, error: FirestoreError): void {
  try {
    onError(error);
  } catch {
    // Observer errors stay user-owned; do not cascade another transport failure.
  }
}
