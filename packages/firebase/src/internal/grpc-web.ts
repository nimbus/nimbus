import { Code, ConnectError, createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";

import { firestoreV1 } from "./protobuf";

export interface FirestoreGrpcWebContext {
  readonly baseUrl: string;
  readonly fetch?: typeof globalThis.fetch;
  readonly apiKey?: string;
  readonly appId?: string;
  readonly headers?: Record<string, string>;
  readonly canRefreshAuthToken: boolean;
  resolveAuthToken(forceRefresh: boolean): Promise<string | null>;
}

export interface FirestoreGrpcWebError {
  readonly code: string;
  readonly message: string;
  readonly status: number;
}

// Unary Commit/BeginTransaction/Rollback plus server-streaming BatchGet/RunQuery
// are available over gRPC-Web. Browser Listen stays on the dedicated WebSocket
// transport.
export async function beginTransactionGrpcWeb(
  context: FirestoreGrpcWebContext,
  request: Parameters<ReturnType<typeof createFirestoreClient>["beginTransaction"]>[0],
) {
  return createFirestoreClient(context).beginTransaction(request);
}

export async function commitGrpcWeb(
  context: FirestoreGrpcWebContext,
  request: Parameters<ReturnType<typeof createFirestoreClient>["commit"]>[0],
) {
  return createFirestoreClient(context).commit(request);
}

export async function batchGetDocumentsGrpcWeb(
  context: FirestoreGrpcWebContext,
  request: Parameters<ReturnType<typeof createFirestoreClient>["batchGetDocuments"]>[0],
) {
  const responses = [];
  for await (const response of createFirestoreClient(context).batchGetDocuments(request)) {
    responses.push(response);
  }
  return responses;
}

export async function runQueryGrpcWeb(
  context: FirestoreGrpcWebContext,
  request: Parameters<ReturnType<typeof createFirestoreClient>["runQuery"]>[0],
) {
  const responses = [];
  for await (const response of createFirestoreClient(context).runQuery(request)) {
    responses.push(response);
  }
  return responses;
}

export async function rollbackGrpcWeb(
  context: FirestoreGrpcWebContext,
  request: Parameters<ReturnType<typeof createFirestoreClient>["rollback"]>[0],
) {
  return createFirestoreClient(context).rollback(request);
}

export function mapGrpcWebError(error: unknown): FirestoreGrpcWebError | null {
  if (!(error instanceof ConnectError)) {
    return null;
  }
  return {
    code: connectCodeName(error.code),
    message: error.rawMessage ?? error.message,
    status: connectCodeStatus(error.code),
  };
}

function createFirestoreClient(context: FirestoreGrpcWebContext) {
  return createClient(
    firestoreV1.Firestore,
    createGrpcWebTransport({
      baseUrl: context.baseUrl,
      fetch: (input, init) => grpcWebFetch(context, input, init),
      useBinaryFormat: true,
    }),
  );
}

async function grpcWebFetch(
  context: FirestoreGrpcWebContext,
  input: Parameters<typeof globalThis.fetch>[0],
  init: Parameters<typeof globalThis.fetch>[1],
) {
  const fetchImpl = context.fetch ?? globalThis.fetch;
  if (typeof fetchImpl !== "function") {
    throw new Error("No fetch implementation is available for Firestore requests.");
  }

  const send = async (forceRefresh: boolean) => {
    const token = await context.resolveAuthToken(forceRefresh);
    const headers = new Headers(init?.headers ?? {});
    if (context.apiKey) {
      headers.set("x-goog-api-key", context.apiKey);
    }
    if (context.appId) {
      headers.set("x-firebase-gmpid", context.appId);
    }
    for (const [key, value] of Object.entries(context.headers ?? {})) {
      headers.set(key, value);
    }
    if (token) {
      headers.set("Authorization", `Bearer ${token}`);
    }
    return fetchImpl(input, {
      ...init,
      headers,
    });
  };

  let response = await send(false);
  if (response.status === 401 && context.canRefreshAuthToken) {
    response = await send(true);
  }
  return response;
}

function connectCodeName(code: Code): string {
  return Code[code].replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase();
}

function connectCodeStatus(code: Code): number {
  switch (code) {
    case Code.Canceled:
      return 499;
    case Code.InvalidArgument:
    case Code.FailedPrecondition:
    case Code.OutOfRange:
      return 400;
    case Code.NotFound:
      return 404;
    case Code.AlreadyExists:
    case Code.Aborted:
      return 409;
    case Code.PermissionDenied:
      return 403;
    case Code.ResourceExhausted:
      return 429;
    case Code.DeadlineExceeded:
      return 504;
    case Code.Unimplemented:
      return 501;
    case Code.Unavailable:
      return 503;
    case Code.Unauthenticated:
      return 401;
    case Code.Unknown:
    case Code.Internal:
    case Code.DataLoss:
    default:
      return 500;
  }
}
