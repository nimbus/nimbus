import type { JsonValue } from "@bufbuild/protobuf";

import type {
  DocumentData,
  DocumentReference,
  DocumentSnapshot,
  Firestore,
  FirestoreError,
  Query,
  QuerySnapshot,
} from "../firestore";
import {
  batchGetDocumentsGrpcWeb,
  beginTransactionGrpcWeb,
  commitGrpcWeb,
  mapGrpcWebError,
  rollbackGrpcWeb,
  runQueryGrpcWeb,
  type FirestoreGrpcWebContext,
} from "./grpc-web";
import { encodeFirestoreValue } from "./document-data";
import { firestoreV1, fromJson, toJson } from "./protobuf";

type QueryStructuredShape = Query<unknown>["structuredQuery"];

type StructuredQueryFilter =
  | {
      readonly fieldFilter: {
        readonly field: { readonly fieldPath: string };
        readonly op: string;
        readonly value: unknown;
      };
    }
  | {
      readonly compositeFilter: {
        readonly op: "AND";
        readonly filters: readonly StructuredQueryFilter[];
      };
    };

type StructuredQueryOrder = {
  readonly field: { readonly fieldPath: string };
  readonly direction: string;
};

type StructuredQueryCursor = {
  readonly before: boolean;
  readonly values: readonly unknown[];
};

export interface FirestoreUnaryDependencies {
  canRefreshAuthToken(firestore: Firestore): boolean;
  resolveAuthToken(firestore: Firestore, forceRefresh: boolean): Promise<string | null>;
  createGrpcWebContext(firestore: Firestore): FirestoreGrpcWebContext;
  createFirestoreError(code: string, message: string, status: number): FirestoreError;
  databaseBaseUrl(firestore: Firestore): string;
  databaseResourceName(firestore: Firestore): string;
  documentResourceName<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): string;
  queryParentResourceName<AppModelType = DocumentData>(query: Query<AppModelType>): string;
  decodeDocumentFields(fields: Record<string, unknown>): DocumentData;
  buildDocumentSnapshot<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    documentData: DocumentData | undefined,
  ): DocumentSnapshot<AppModelType>;
  buildQuerySnapshot<AppModelType = DocumentData>(
    query: Query<AppModelType>,
    documents: readonly { name: string; documentData: DocumentData }[],
  ): QuerySnapshot<AppModelType>;
}

export function encodeStructuredQueryForTransport(
  queryShape: QueryStructuredShape,
): Record<string, unknown> {
  return {
    ...(queryShape.endAt
      ? {
          endAt: encodeStructuredCursorForTransport(
            queryShape.endAt,
            queryShape.orderBy,
          ),
        }
      : {}),
    from: queryShape.from.map((selector) => ({
      ...(selector.allDescendants ? { allDescendants: true } : {}),
      collectionId: selector.collectionId,
    })),
    ...(queryShape.limit !== undefined ? { limit: queryShape.limit } : {}),
    ...(queryShape.orderBy
      ? {
          orderBy: queryShape.orderBy.map((order) => ({
            direction: order.direction,
            field: { fieldPath: order.field.fieldPath },
          })),
        }
      : {}),
    ...(queryShape.startAt
      ? {
          startAt: encodeStructuredCursorForTransport(
            queryShape.startAt,
            queryShape.orderBy,
          ),
        }
      : {}),
    ...(queryShape.where
      ? { where: encodeStructuredQueryFilterForTransport(queryShape.where) }
      : {}),
  };
}

export async function commitWritesInternal(
  firestore: Firestore,
  writes: readonly Record<string, unknown>[],
  options: { transaction?: Uint8Array } | undefined,
  dependencies: FirestoreUnaryDependencies,
): Promise<void> {
  const body = {
    database: dependencies.databaseResourceName(firestore),
    ...(options?.transaction ? { transaction: encodeBase64(options.transaction) } : {}),
    writes: [...writes],
  };
  if (usesGrpcWebUnaryTransport(firestore)) {
    try {
      await commitGrpcWeb(
        dependencies.createGrpcWebContext(firestore),
        fromJson(firestoreV1.CommitRequestSchema, body as JsonValue),
      );
      return;
    } catch (error) {
      throw mapTransportError(error, dependencies);
    }
  }
  const response = await performFirestoreRequest(
    firestore,
    "/documents:commit",
    body,
    dependencies,
  );
  if (!response.ok) {
    throw await parseFirestoreError(response, dependencies);
  }
  await response.json();
}

export async function batchGetDocumentInternal<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  options: { transaction?: Uint8Array } | undefined,
  dependencies: FirestoreUnaryDependencies,
): Promise<DocumentSnapshot<AppModelType>> {
  const body = {
    database: dependencies.databaseResourceName(reference.firestore),
    documents: [dependencies.documentResourceName(reference)],
    ...(options?.transaction ? { transaction: encodeBase64(options.transaction) } : {}),
  };
  if (usesGrpcWebUnaryTransport(reference.firestore)) {
    try {
      const responses = await batchGetDocumentsGrpcWeb(
        dependencies.createGrpcWebContext(reference.firestore),
        fromJson(firestoreV1.BatchGetDocumentsRequestSchema, body as JsonValue),
      );
      return decodeBatchGetDocumentEntries(
        reference,
        responses.map((response) =>
          toJson(firestoreV1.BatchGetDocumentsResponseSchema, response),
        ),
        dependencies,
      );
    } catch (error) {
      throw mapTransportError(error, dependencies);
    }
  }
  const response = await performFirestoreRequest(
    reference.firestore,
    "/documents:batchGet",
    body,
    dependencies,
  );
  if (!response.ok) {
    throw await parseFirestoreError(response, dependencies);
  }
  return decodeBatchGetDocumentEntries(
    reference,
    parseJsonLines(await response.text()),
    dependencies,
  );
}

export async function beginFirestoreTransactionInternal(
  firestore: Firestore,
  dependencies: FirestoreUnaryDependencies,
): Promise<Uint8Array> {
  const body = {
    database: dependencies.databaseResourceName(firestore),
  };
  if (usesGrpcWebUnaryTransport(firestore)) {
    try {
      const response = await beginTransactionGrpcWeb(
        dependencies.createGrpcWebContext(firestore),
        fromJson(firestoreV1.BeginTransactionRequestSchema, body as JsonValue),
      );
      return response.transaction;
    } catch (error) {
      throw mapTransportError(error, dependencies);
    }
  }
  const response = await performFirestoreRequest(
    firestore,
    "/documents:beginTransaction",
    body,
    dependencies,
  );
  if (!response.ok) {
    throw await parseFirestoreError(response, dependencies);
  }
  const payload = (await response.json()) as { transaction?: string };
  if (typeof payload.transaction !== "string") {
    throw new Error("BeginTransaction returned an unexpected response shape.");
  }
  return decodeBase64(payload.transaction);
}

export async function rollbackFirestoreTransactionInternal(
  firestore: Firestore,
  transaction: Uint8Array,
  dependencies: FirestoreUnaryDependencies,
): Promise<void> {
  const body = {
    database: dependencies.databaseResourceName(firestore),
    transaction: encodeBase64(transaction),
  };
  if (usesGrpcWebUnaryTransport(firestore)) {
    try {
      await rollbackGrpcWeb(
        dependencies.createGrpcWebContext(firestore),
        fromJson(firestoreV1.RollbackRequestSchema, body as JsonValue),
      );
      return;
    } catch (error) {
      throw mapTransportError(error, dependencies);
    }
  }
  const response = await performFirestoreRequest(
    firestore,
    "/documents:rollback",
    body,
    dependencies,
  );
  if (!response.ok) {
    throw await parseFirestoreError(response, dependencies);
  }
}

export async function runQueryDocumentsInternal<AppModelType = DocumentData>(
  query: Query<AppModelType>,
  options: { transaction?: Uint8Array } | undefined,
  dependencies: FirestoreUnaryDependencies,
): Promise<QuerySnapshot<AppModelType>> {
  const body = {
    parent: dependencies.queryParentResourceName(query),
    ...(options?.transaction ? { transaction: encodeBase64(options.transaction) } : {}),
    structuredQuery: encodeStructuredQueryForTransport(query.structuredQuery),
  };
  if (usesGrpcWebUnaryTransport(query.firestore)) {
    try {
      const responses = await runQueryGrpcWeb(
        dependencies.createGrpcWebContext(query.firestore),
        fromJson(firestoreV1.RunQueryRequestSchema, body as JsonValue),
      );
      return decodeRunQueryEntries(
        query,
        responses.map((response) => toJson(firestoreV1.RunQueryResponseSchema, response)),
        dependencies,
      );
    } catch (error) {
      throw mapTransportError(error, dependencies);
    }
  }
  const response = await performFirestoreRequest(
    query.firestore,
    queryRouteSuffix(query),
    body,
    dependencies,
  );
  if (!response.ok) {
    throw await parseFirestoreError(response, dependencies);
  }
  return decodeRunQueryEntries(
    query,
    parseJsonLines(await response.text()),
    dependencies,
  );
}

function encodeQueryTransportValue(fieldPath: string, value: unknown): Record<string, unknown> {
  if (fieldPath === "__name__" && typeof value === "string") {
    return { referenceValue: value };
  }
  return encodeFirestoreValue(value);
}

function encodeStructuredQueryFilterForTransport(
  filter: StructuredQueryFilter | undefined,
): Record<string, unknown> | undefined {
  if (!filter) {
    return undefined;
  }
  if ("fieldFilter" in filter) {
    return {
      fieldFilter: {
        field: { fieldPath: filter.fieldFilter.field.fieldPath },
        op: filter.fieldFilter.op,
        value: encodeQueryTransportValue(
          filter.fieldFilter.field.fieldPath,
          filter.fieldFilter.value,
        ),
      },
    };
  }
  return {
    compositeFilter: {
      op: filter.compositeFilter.op,
      filters: filter.compositeFilter.filters.map((entry) =>
        encodeStructuredQueryFilterForTransport(entry),
      ),
    },
  };
}

function encodeStructuredCursorForTransport(
  cursor: StructuredQueryCursor | undefined,
  orderBy: readonly StructuredQueryOrder[] | undefined,
): Record<string, unknown> | undefined {
  if (!cursor) {
    return undefined;
  }
  return {
    before: cursor.before,
    values: cursor.values.map((value, index) =>
      encodeQueryTransportValue(orderBy?.[index]?.field.fieldPath ?? "", value),
    ),
  };
}

function mapTransportError(
  error: unknown,
  dependencies: FirestoreUnaryDependencies,
): FirestoreError {
  const grpcWebError = mapGrpcWebError(error);
  if (grpcWebError) {
    return dependencies.createFirestoreError(
      grpcWebError.code,
      grpcWebError.message,
      grpcWebError.status,
    );
  }
  if (error instanceof Error) {
    throw error;
  }
  throw new Error(String(error));
}

async function performFirestoreRequest(
  firestore: Firestore,
  suffix: string,
  body: unknown,
  dependencies: FirestoreUnaryDependencies,
): Promise<Response> {
  const fetchImpl = firestore.settings.experimentalFetch ?? globalThis.fetch;
  if (typeof fetchImpl !== "function") {
    throw new Error("No fetch implementation is available for Firestore requests.");
  }

  const send = async (token: string | null) => {
    const headers = new Headers({
      Accept: "application/json",
      "Content-Type": "text/plain;charset=UTF-8",
      ...(firestore.settings.experimentalHeaders ?? {}),
    });
    if (firestore.app.options.apiKey) {
      headers.set("x-goog-api-key", firestore.app.options.apiKey);
    }
    if (firestore.app.options.appId) {
      headers.set("x-firebase-gmpid", firestore.app.options.appId);
    }
    if (token) {
      headers.set("Authorization", `Bearer ${token}`);
    }
    return fetchImpl(`${dependencies.databaseBaseUrl(firestore)}${suffix}`, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
    });
  };

  let token = await dependencies.resolveAuthToken(firestore, false);
  let response = await send(token);
  if (response.status === 401 && dependencies.canRefreshAuthToken(firestore)) {
    token = await dependencies.resolveAuthToken(firestore, true);
    response = await send(token);
  }
  return response;
}

async function parseFirestoreError(
  response: Response,
  dependencies: FirestoreUnaryDependencies,
): Promise<FirestoreError> {
  const bodyText = await response.text();
  try {
    const parsed = JSON.parse(bodyText) as {
      error?: { status?: string; message?: string };
    };
    const code = parsed.error?.status ?? "UNKNOWN";
    const message =
      parsed.error?.message ??
      `Firestore request failed with HTTP ${response.status}.`;
    return dependencies.createFirestoreError(code, message, response.status);
  } catch {
    return dependencies.createFirestoreError(
      "UNKNOWN",
      bodyText || `Firestore request failed with HTTP ${response.status}.`,
      response.status,
    );
  }
}

function parseJsonLines(body: string): unknown[] {
  return body
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line) as unknown);
}

function decodeBatchGetDocumentEntries<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  lines: readonly unknown[],
  dependencies: FirestoreUnaryDependencies,
): DocumentSnapshot<AppModelType> {
  if (lines.length === 0) {
    throw new Error("BatchGetDocuments returned no response entries.");
  }

  const entry = lines[0] as {
    found?: { fields?: Record<string, unknown> };
    missing?: string;
  };
  if (entry.missing) {
    return dependencies.buildDocumentSnapshot(reference, undefined);
  }
  if (!entry.found) {
    throw new Error("BatchGetDocuments returned an unexpected response shape.");
  }
  const documentData = dependencies.decodeDocumentFields(entry.found.fields ?? {});
  return dependencies.buildDocumentSnapshot(reference, documentData);
}

function decodeRunQueryEntries<AppModelType = DocumentData>(
  query: Query<AppModelType>,
  entries: readonly unknown[],
  dependencies: FirestoreUnaryDependencies,
): QuerySnapshot<AppModelType> {
  const documents = (entries as Array<{
    document?: {
      fields?: Record<string, unknown>;
      name: string;
    };
  }>)
    .filter((entry) => entry.document !== undefined)
    .map((entry) => {
      const document = entry.document as {
        fields?: Record<string, unknown>;
        name: string;
      };
      return {
        name: document.name,
        documentData: dependencies.decodeDocumentFields(document.fields ?? {}),
      };
    });
  return dependencies.buildQuerySnapshot(query, documents);
}

function queryRouteSuffix<AppModelType = DocumentData>(query: Query<AppModelType>): string {
  if (query.source.type === "collection" && query.source.parent) {
    return `/documents/${encodedPath(query.source.parent.path)}:runQuery`;
  }
  return "/documents:runQuery";
}

function usesGrpcWebUnaryTransport(firestore: Firestore): boolean {
  return firestore.settings.experimentalUnaryTransport === "grpc-web";
}

function encodedPath(path: string): string {
  return path
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/");
}

function encodeBase64(bytes: Uint8Array): string {
  const bufferCtor = (globalThis as {
    Buffer?: {
      from(bytes: Uint8Array): {
        toString(encoding: "base64"): string;
      };
    };
  }).Buffer;
  if (bufferCtor) {
    return bufferCtor.from(bytes).toString("base64");
  }
  const encode = globalThis.btoa;
  if (typeof encode !== "function") {
    throw new Error("No base64 encoder is available for Firestore transaction bytes.");
  }
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return encode(binary);
}

function decodeBase64(value: string): Uint8Array {
  const bufferCtor = (globalThis as {
    Buffer?: {
      from(value: string, encoding: "base64"): Uint8Array;
    };
  }).Buffer;
  if (bufferCtor) {
    return new Uint8Array(bufferCtor.from(value, "base64"));
  }
  const decode = globalThis.atob;
  if (typeof decode !== "function") {
    throw new Error("No base64 decoder is available for Firestore transaction bytes.");
  }
  const binary = decode(value);
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}
