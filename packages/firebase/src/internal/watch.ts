import type { JsonValue } from "@bufbuild/protobuf";

import {
  openFirestoreListenWebSocket,
  type FirestoreListenResumeCursor,
} from "./listen-websocket";
import { create, firestoreQueryV1, firestoreV1, fromJson } from "./protobuf";

import type {
  CollectionGroup,
  CollectionReference,
  DocumentData,
  DocumentReference,
  DocumentSnapshot,
  Firestore,
  FirestoreError,
  Query,
  QuerySnapshot,
  SnapshotObserver,
  Unsubscribe,
} from "../firestore";

export type SnapshotListenSource<AppModelType = DocumentData> =
  | DocumentReference<AppModelType>
  | CollectionReference<AppModelType>
  | CollectionGroup<AppModelType>
  | Query<AppModelType>;

export type SnapshotForSource<Source> = Source extends DocumentReference<infer AppModelType>
  ? DocumentSnapshot<AppModelType>
  : Source extends
        | CollectionReference<infer AppModelType>
        | CollectionGroup<infer AppModelType>
        | Query<infer AppModelType>
    ? QuerySnapshot<AppModelType>
    : never;

type TerminationHookOwner = {
  addTerminationHook(hook: () => void): void;
  removeTerminationHook(hook: () => void): void;
};

export interface FirestoreWatchDependencies {
  databaseResourceName(firestore: Firestore): string;
  documentResourceName<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): string;
  queryParentResourceName<AppModelType = DocumentData>(
    query: Query<AppModelType>,
  ): string;
  encodeStructuredQueryForTransport(structuredQuery: unknown): Record<string, unknown>;
  canRefreshAuthToken(firestore: Firestore): boolean;
  listenWebSocketUrl(firestore: Firestore): string;
  resolveListenWebSocketSubprotocols(
    firestore: Firestore,
    forceRefresh: boolean,
  ): Promise<readonly string[]>;
  firestoreImpl(firestore: Firestore): TerminationHookOwner;
  isDocumentReference(value: unknown): value is DocumentReference<unknown>;
  normalizeQuerySource<AppModelType = DocumentData>(
    source:
      | CollectionReference<AppModelType>
      | CollectionGroup<AppModelType>
      | Query<AppModelType>,
  ): Query<AppModelType>;
  isPlainObject(value: unknown): value is Record<string, unknown>;
  decodeDocumentFields(fields: Record<string, unknown>): DocumentData;
  buildDocumentSnapshot<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    documentData: DocumentData | undefined,
  ): DocumentSnapshot<AppModelType>;
  buildQuerySnapshot<AppModelType = DocumentData>(
    query: Query<AppModelType>,
    documents: Map<string, DocumentData>,
  ): QuerySnapshot<AppModelType>;
  createFirestoreError(code: string, message: string, status: number): FirestoreError;
}

function buildListenAddDocumentRequest<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  targetId: number,
  cursor: FirestoreListenResumeCursor | null,
  dependencies: FirestoreWatchDependencies,
) {
  return create(firestoreV1.ListenRequestSchema, {
    database: dependencies.databaseResourceName(reference.firestore),
    targetChange: {
      case: "addTarget",
      value: {
        resumeType: buildListenResumeType(cursor),
        targetId,
        targetType: {
          case: "documents",
          value: {
            documents: [dependencies.documentResourceName(reference)],
          },
        },
      },
    },
  });
}

function buildListenAddQueryRequest<AppModelType = DocumentData>(
  query: Query<AppModelType>,
  targetId: number,
  cursor: FirestoreListenResumeCursor | null,
  dependencies: FirestoreWatchDependencies,
) {
  return create(firestoreV1.ListenRequestSchema, {
    database: dependencies.databaseResourceName(query.firestore),
    targetChange: {
      case: "addTarget",
      value: {
        resumeType: buildListenResumeType(cursor),
        targetId,
        targetType: {
          case: "query",
          value: {
            parent: dependencies.queryParentResourceName(query),
            queryType: {
              case: "structuredQuery",
              value: fromJson(
                firestoreQueryV1.StructuredQuerySchema,
                dependencies.encodeStructuredQueryForTransport(
                  query.structuredQuery,
                ) as JsonValue,
              ),
            },
          },
        },
      },
    },
  });
}

function buildListenRemoveTargetRequest(
  firestore: Firestore,
  targetId: number,
  dependencies: FirestoreWatchDependencies,
) {
  return create(firestoreV1.ListenRequestSchema, {
    database: dependencies.databaseResourceName(firestore),
    targetChange: {
      case: "removeTarget",
      value: targetId,
    },
  });
}

function buildListenResumeType(cursor: FirestoreListenResumeCursor | null) {
  if (cursor?.resumeToken && cursor.resumeToken.byteLength > 0) {
    return {
      case: "resumeToken" as const,
      value: new Uint8Array(cursor.resumeToken),
    };
  }
  if (cursor?.readTime) {
    return {
      case: "readTime" as const,
      value: {
        seconds: cursor.readTime.seconds,
        nanos: cursor.readTime.nanos,
      },
    };
  }
  return { case: undefined };
}

function normalizeSnapshotObserver<SnapshotType>(
  observerOrNext:
    | SnapshotObserver<SnapshotType>
    | ((snapshot: SnapshotType) => void),
  onError?: (error: FirestoreError) => void,
  onCompletion?: () => void,
): SnapshotObserver<SnapshotType> {
  if (typeof observerOrNext === "function") {
    return {
      next: observerOrNext,
      error: onError,
      complete: onCompletion,
    };
  }
  return observerOrNext;
}

function listenTargetChangeError(
  change: Record<string, unknown>,
  dependencies: FirestoreWatchDependencies,
): FirestoreError | null {
  const cause =
    dependencies.isPlainObject(change.cause) &&
    typeof change.cause.message === "string"
      ? (change.cause as { code?: unknown; message: string })
      : null;
  if (!cause) {
    return null;
  }
  const grpcCode = typeof cause.code === "number" ? cause.code : 2;
  return dependencies.createFirestoreError(
    grpcStatusName(grpcCode),
    cause.message,
    grpcStatusHttpStatus(grpcCode),
  );
}

function grpcStatusName(code: number): string {
  switch (code) {
    case 1:
      return "CANCELLED";
    case 3:
      return "INVALID_ARGUMENT";
    case 4:
      return "DEADLINE_EXCEEDED";
    case 5:
      return "NOT_FOUND";
    case 6:
      return "ALREADY_EXISTS";
    case 7:
      return "PERMISSION_DENIED";
    case 8:
      return "RESOURCE_EXHAUSTED";
    case 9:
      return "FAILED_PRECONDITION";
    case 10:
      return "ABORTED";
    case 11:
      return "OUT_OF_RANGE";
    case 12:
      return "UNIMPLEMENTED";
    case 13:
      return "INTERNAL";
    case 14:
      return "UNAVAILABLE";
    case 15:
      return "DATA_LOSS";
    case 16:
      return "UNAUTHENTICATED";
    case 2:
    default:
      return "UNKNOWN";
  }
}

function grpcStatusHttpStatus(code: number): number {
  switch (code) {
    case 1:
      return 499;
    case 3:
    case 9:
    case 11:
      return 400;
    case 5:
      return 404;
    case 6:
    case 10:
      return 409;
    case 7:
      return 403;
    case 8:
      return 429;
    case 4:
      return 504;
    case 12:
      return 501;
    case 14:
      return 503;
    case 16:
      return 401;
    case 2:
    case 13:
    case 15:
    default:
      return 500;
  }
}

export function onSnapshotInternal<Source extends SnapshotListenSource>(
  source: Source,
  observerOrNext:
    | SnapshotObserver<SnapshotForSource<Source>>
    | ((snapshot: SnapshotForSource<Source>) => void),
  onError: ((error: FirestoreError) => void) | undefined,
  onCompletion: (() => void) | undefined,
  dependencies: FirestoreWatchDependencies,
): Unsubscribe {
  const firestore = source.firestore;
  const observer = normalizeSnapshotObserver(
    observerOrNext as
      | SnapshotObserver<SnapshotForSource<Source>>
      | ((snapshot: SnapshotForSource<Source>) => void),
    onError,
    onCompletion,
  );
  const targetId = 1;
  const removeTargetRequest = buildListenRemoveTargetRequest(
    firestore,
    targetId,
    dependencies,
  );

  const fail = (error: FirestoreError) => {
    if (typeof observer.error === "function") {
      observer.error(error);
      return;
    }
    throw error;
  };

  if (dependencies.isDocumentReference(source)) {
    const reference = source as DocumentReference<unknown>;
    let bootstrapReady = false;
    let unsubscribed = false;
    let documentData: DocumentData | undefined;
    const session = openFirestoreListenWebSocket({
      canRefreshAuthToken: dependencies.canRefreshAuthToken(firestore),
      socketFactory: firestore.settings.experimentalWebSocketFactory,
      url: dependencies.listenWebSocketUrl(firestore),
      targetId,
      resolveSubprotocols: (forceRefresh) =>
        dependencies.resolveListenWebSocketSubprotocols(firestore, forceRefresh),
      buildAddTargetRequest: (cursor) =>
        buildListenAddDocumentRequest(reference, targetId, cursor, dependencies),
      removeTargetRequest,
      onError: (error) => {
        if (unsubscribed) {
          return;
        }
        unsubscribed = true;
        dependencies.firestoreImpl(firestore).removeTerminationHook(unsubscribe);
        fail(error);
      },
      onReconnect: () => {
        if (!unsubscribed) {
          bootstrapReady = false;
        }
      },
      onResponse: (response) => {
        if (!dependencies.isPlainObject(response)) {
          return;
        }
        if (dependencies.isPlainObject(response.documentChange)) {
          const change = response.documentChange;
          const targetIds = Array.isArray(change.targetIds) ? change.targetIds : [];
          const removedTargetIds = Array.isArray(change.removedTargetIds)
            ? change.removedTargetIds
            : [];
          const document = dependencies.isPlainObject(change.document)
            ? (change.document as Record<string, unknown>)
            : null;
          const documentName =
            document && typeof document.name === "string" ? document.name : null;
          if (
            removedTargetIds.includes(targetId) &&
            documentName === dependencies.documentResourceName(reference)
          ) {
            documentData = undefined;
          }
          if (targetIds.includes(targetId) && document !== null) {
            documentData = dependencies.decodeDocumentFields(
              dependencies.isPlainObject(document.fields)
                ? (document.fields as Record<string, unknown>)
                : {},
            );
          }
          if (bootstrapReady && typeof observer.next === "function") {
            observer.next(
              dependencies.buildDocumentSnapshot(
                reference,
                documentData,
              ) as SnapshotForSource<Source>,
            );
          }
          return;
        }

        if (
          dependencies.isPlainObject(response.documentDelete) ||
          dependencies.isPlainObject(response.documentRemove)
        ) {
          const change = (
            dependencies.isPlainObject(response.documentDelete)
              ? response.documentDelete
              : response.documentRemove
          ) as Record<string, unknown>;
          const removedTargetIds = Array.isArray(change.removedTargetIds)
            ? change.removedTargetIds
            : [];
          const documentName = typeof change.document === "string" ? change.document : null;
          if (
            removedTargetIds.includes(targetId) &&
            documentName === dependencies.documentResourceName(reference)
          ) {
            documentData = undefined;
            if (bootstrapReady && typeof observer.next === "function") {
              observer.next(
                dependencies.buildDocumentSnapshot(
                  reference,
                  documentData,
                ) as SnapshotForSource<Source>,
              );
            }
          }
          return;
        }

        if (dependencies.isPlainObject(response.targetChange)) {
          const change = response.targetChange as Record<string, unknown>;
          const targetIds = Array.isArray(change.targetIds) ? change.targetIds : [];
          const changeType =
            typeof change.targetChangeType === "string" ? change.targetChangeType : null;
          if (
            targetIds.length > 0 &&
            !targetIds.includes(targetId) &&
            changeType !== "NO_CHANGE"
          ) {
            return;
          }
          if (changeType === "REMOVE") {
            unsubscribe();
            fail(
              listenTargetChangeError(change, dependencies) ??
                dependencies.createFirestoreError(
                  "ABORTED",
                  "Firestore Listen target was removed by the server.",
                  409,
                ),
            );
            return;
          }
          if (changeType === "RESET") {
            unsubscribe();
            fail(
              dependencies.createFirestoreError(
                "ABORTED",
                "Firestore Listen target requested a reset before resume support is enabled.",
                409,
              ),
            );
            return;
          }
          if (changeType === "CURRENT" && !bootstrapReady && typeof observer.next === "function") {
            bootstrapReady = true;
            observer.next(
              dependencies.buildDocumentSnapshot(
                reference,
                documentData,
              ) as SnapshotForSource<Source>,
            );
          }
        }
      },
    });

    const unsubscribe = () => {
      if (unsubscribed) {
        return;
      }
      unsubscribed = true;
      dependencies.firestoreImpl(firestore).removeTerminationHook(unsubscribe);
      session.close();
    };
    dependencies.firestoreImpl(firestore).addTerminationHook(unsubscribe);
    return unsubscribe;
  }

  const queryTarget = dependencies.normalizeQuerySource(
    source as CollectionReference<unknown> | CollectionGroup<unknown> | Query<unknown>,
  );
  let bootstrapReady = false;
  let unsubscribed = false;
  const queryDocuments = new Map<string, DocumentData>();

  const session = openFirestoreListenWebSocket({
    canRefreshAuthToken: dependencies.canRefreshAuthToken(firestore),
    socketFactory: firestore.settings.experimentalWebSocketFactory,
    url: dependencies.listenWebSocketUrl(firestore),
    targetId,
    resolveSubprotocols: (forceRefresh) =>
      dependencies.resolveListenWebSocketSubprotocols(firestore, forceRefresh),
    buildAddTargetRequest: (cursor) =>
      buildListenAddQueryRequest(queryTarget, targetId, cursor, dependencies),
    removeTargetRequest,
    onError: (error) => {
      if (unsubscribed) {
        return;
      }
      unsubscribed = true;
      dependencies.firestoreImpl(firestore).removeTerminationHook(unsubscribe);
      fail(error);
    },
    onReconnect: () => {
      if (!unsubscribed) {
        bootstrapReady = false;
      }
    },
    onResponse: (response) => {
      if (!dependencies.isPlainObject(response)) {
        return;
      }
      if (dependencies.isPlainObject(response.documentChange)) {
        const change = response.documentChange;
        const targetIds = Array.isArray(change.targetIds) ? change.targetIds : [];
        const removedTargetIds = Array.isArray(change.removedTargetIds)
          ? change.removedTargetIds
          : [];
        const document = dependencies.isPlainObject(change.document)
          ? (change.document as Record<string, unknown>)
          : null;
        const documentName =
          document && typeof document.name === "string" ? document.name : null;
        if (!documentName) {
          return;
        }
        if (removedTargetIds.includes(targetId)) {
          queryDocuments.delete(documentName);
        }
        if (targetIds.includes(targetId) && document !== null) {
          queryDocuments.set(
            documentName,
            dependencies.decodeDocumentFields(
              dependencies.isPlainObject(document.fields)
                ? (document.fields as Record<string, unknown>)
                : {},
            ),
          );
        }
        if (bootstrapReady && typeof observer.next === "function") {
          observer.next(
            dependencies.buildQuerySnapshot(
              queryTarget,
              queryDocuments,
            ) as SnapshotForSource<Source>,
          );
        }
        return;
      }

      if (
        dependencies.isPlainObject(response.documentDelete) ||
        dependencies.isPlainObject(response.documentRemove)
      ) {
        const change = (
          dependencies.isPlainObject(response.documentDelete)
            ? response.documentDelete
            : response.documentRemove
        ) as Record<string, unknown>;
        const removedTargetIds = Array.isArray(change.removedTargetIds)
          ? change.removedTargetIds
          : [];
        if (!removedTargetIds.includes(targetId)) {
          return;
        }
        const documentName = typeof change.document === "string" ? change.document : null;
        if (documentName) {
          queryDocuments.delete(documentName);
          if (bootstrapReady && typeof observer.next === "function") {
            observer.next(
              dependencies.buildQuerySnapshot(
                queryTarget,
                queryDocuments,
              ) as SnapshotForSource<Source>,
            );
          }
        }
        return;
      }

      if (dependencies.isPlainObject(response.targetChange)) {
        const change = response.targetChange;
        const targetIds = Array.isArray(change.targetIds) ? change.targetIds : [];
        if (
          targetIds.length > 0 &&
          !targetIds.includes(targetId) &&
          change.targetChangeType !== "NO_CHANGE"
        ) {
          return;
        }
        if (change.targetChangeType === "REMOVE") {
          unsubscribe();
          fail(
            listenTargetChangeError(change, dependencies) ??
              dependencies.createFirestoreError(
                "ABORTED",
                "Firestore Listen target was removed by the server.",
                409,
              ),
          );
          return;
        }
        if (change.targetChangeType === "RESET") {
          unsubscribe();
          fail(
            dependencies.createFirestoreError(
              "ABORTED",
              "Firestore Listen target requested a reset before resume support is enabled.",
              409,
            ),
          );
          return;
        }
        if (change.targetChangeType === "CURRENT" && !bootstrapReady) {
          bootstrapReady = true;
          if (typeof observer.next === "function") {
            observer.next(
              dependencies.buildQuerySnapshot(
                queryTarget,
                queryDocuments,
              ) as SnapshotForSource<Source>,
            );
          }
        }
      }
    },
  });

  const unsubscribe = () => {
    if (unsubscribed) {
      return;
    }
    unsubscribed = true;
    dependencies.firestoreImpl(firestore).removeTerminationHook(unsubscribe);
    session.close();
  };
  dependencies.firestoreImpl(firestore).addTerminationHook(unsubscribe);
  return unsubscribe;
}
