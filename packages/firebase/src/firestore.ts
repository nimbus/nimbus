import type { FirebaseApp } from "./app";
import {
  FieldValue,
  arrayRemove,
  arrayUnion,
  assertDocumentData,
  decodeDocumentFields,
  deleteField,
  encodeFirestoreValue,
  increment,
  isPlainObject,
  readValueAtFieldPath,
  serverTimestamp,
  splitFieldPath,
} from "./internal/document-data";
import type { FirestoreWebSocketFactory } from "./internal/listen-websocket";
import {
  buildCreateWrite as buildCreateWriteInternal,
  buildDeleteWrite as buildDeleteWriteInternal,
  buildSetWrite as buildSetWriteInternal,
  buildUpdateWrite as buildUpdateWriteInternal,
  type FirestoreWriteDependencies,
} from "./internal/writes";
import {
  beginFirestoreTransactionInternal,
  batchGetDocumentInternal,
  commitWritesInternal,
  encodeStructuredQueryForTransport,
  rollbackFirestoreTransactionInternal,
  runQueryDocumentsInternal,
  type FirestoreUnaryDependencies,
} from "./internal/unary";
import {
  canRefreshAuthToken,
  grpcWebContext,
  resolveAuthToken,
  resolveListenWebSocketSubprotocols,
  type FirestoreAuthDependencies,
} from "./internal/auth";
import {
  applyQueryConstraint,
  assertCollectionId,
  assertCollectionPath,
  assertCursorValues,
  assertDocumentPath,
  assertQueryFieldPath,
  baseStructuredQuery,
  cloneStructuredQueryShape,
  composePath,
  createAutoId,
  databaseBaseUrl,
  databaseResourceName,
  deepEqualValue,
  firestoreFromSource,
  firestoreIdentityEqual,
  isCollectionGroup,
  isCollectionReference,
  isDocumentIdFieldPath,
  isDocumentReference,
  isDocumentSnapshot,
  isQuery,
  isQuerySnapshot,
  queryRouteSuffix,
  resolveAppAndDatabase,
  toFirestoreDocumentData,
  type CursorQueryConstraint,
  type LimitQueryConstraint,
  type OrderByQueryConstraint,
  type QuerySource,
  type StructuredQueryShape,
  type WhereQueryConstraint,
} from "./internal/firestore-helpers";
import {
  onSnapshotInternal,
  type SnapshotForSource,
  type SnapshotListenSource,
} from "./internal/watch";
import {
  buildWatchDocumentSnapshot as buildWatchDocumentSnapshotInternal,
  buildWatchQuerySnapshot as buildWatchQuerySnapshotInternal,
  type FirestoreWatchSnapshotDependencies,
} from "./internal/watch-snapshots";
import { create, firestoreQueryV1, firestoreV1, fromJson, toJson } from "./internal/protobuf";

const DEFAULT_DATABASE_ID = "(default)";
const DEFAULT_HOST = "firestore.googleapis.com";
const DEFAULT_SNAPSHOT_METADATA = Object.freeze({
  fromCache: false,
  hasPendingWrites: false,
});

export {
  FieldValue,
  arrayRemove,
  arrayUnion,
  deleteField,
  increment,
  serverTimestamp,
};

export type DocumentData = Record<string, unknown>;
export type FetchLike = typeof globalThis.fetch;
export type FirestoreUnaryTransport = "rest" | "grpc-web";
export type FirestoreAuthTokenFetcher = (args: {
  forceRefresh: boolean;
}) => Promise<string | null | undefined>;

export interface FirestoreSettings {
  host?: string;
  ssl?: boolean;
  ignoreUndefinedProperties?: boolean;
  experimentalForceLongPolling?: boolean;
  experimentalAutoDetectLongPolling?: boolean;
  useFetchStreams?: boolean;
  experimentalFetch?: FetchLike;
  experimentalWebSocketFactory?: FirestoreWebSocketFactory;
  experimentalHeaders?: Record<string, string>;
  experimentalAuthToken?: string | FirestoreAuthTokenFetcher;
  experimentalUnaryTransport?: FirestoreUnaryTransport;
}

export interface FirestoreEmulatorOptions {
  mockUserToken?: object | string;
}

export interface Firestore {
  readonly app: FirebaseApp;
  readonly databaseId: string;
  readonly settings: Readonly<FirestoreSettings>;
}

export interface SnapshotMetadata {
  readonly fromCache: boolean;
  readonly hasPendingWrites: boolean;
}

export interface FirestoreDataConverter<AppModelType = DocumentData> {
  toFirestore(modelObject: AppModelType): DocumentData;
  fromFirestore(snapshot: QueryDocumentSnapshot<DocumentData>): AppModelType;
}

export interface DocumentReference<AppModelType = DocumentData> {
  readonly converter: FirestoreDataConverter<AppModelType> | null;
  readonly firestore: Firestore;
  readonly id: string;
  readonly parent: CollectionReference<AppModelType>;
  readonly path: string;
  readonly type: "document";
  withConverter(converter: null): DocumentReference<DocumentData>;
  withConverter<NewAppModelType>(
    converter: FirestoreDataConverter<NewAppModelType>,
  ): DocumentReference<NewAppModelType>;
}

export interface CollectionReference<AppModelType = DocumentData> {
  readonly converter: FirestoreDataConverter<AppModelType> | null;
  readonly firestore: Firestore;
  readonly id: string;
  readonly parent: DocumentReference<AppModelType> | null;
  readonly path: string;
  readonly type: "collection";
  withConverter(converter: null): CollectionReference<DocumentData>;
  withConverter<NewAppModelType>(
    converter: FirestoreDataConverter<NewAppModelType>,
  ): CollectionReference<NewAppModelType>;
}

export interface CollectionGroup<AppModelType = DocumentData> {
  readonly converter: FirestoreDataConverter<AppModelType> | null;
  readonly firestore: Firestore;
  readonly id: string;
  readonly type: "collectionGroup";
  withConverter(converter: null): CollectionGroup<DocumentData>;
  withConverter<NewAppModelType>(
    converter: FirestoreDataConverter<NewAppModelType>,
  ): CollectionGroup<NewAppModelType>;
}

export interface Query<AppModelType = DocumentData> {
  readonly converter: FirestoreDataConverter<AppModelType> | null;
  readonly firestore: Firestore;
  readonly source: QuerySource<AppModelType>;
  readonly structuredQuery: Readonly<StructuredQueryShape>;
  readonly type: "query";
  withConverter(converter: null): Query<DocumentData>;
  withConverter<NewAppModelType>(
    converter: FirestoreDataConverter<NewAppModelType>,
  ): Query<NewAppModelType>;
}

export interface DocumentIdFieldPath {
  readonly type: "documentIdFieldPath";
}

export type QueryConstraint =
  | WhereQueryConstraint
  | OrderByQueryConstraint
  | LimitQueryConstraint
  | CursorQueryConstraint;

export type WhereFilterOp =
  | "<"
  | "<="
  | "=="
  | "!="
  | ">="
  | ">"
  | "array-contains"
  | "in"
  | "array-contains-any"
  | "not-in";

export type OrderByDirection = "asc" | "desc";

export interface DocumentSnapshot<AppModelType = DocumentData> {
  readonly id: string;
  readonly metadata: SnapshotMetadata;
  readonly ref: DocumentReference<AppModelType>;
  exists(): boolean;
  data(): AppModelType | undefined;
  get(fieldPath: string): unknown;
}

export interface QueryDocumentSnapshot<AppModelType = DocumentData>
  extends DocumentSnapshot<AppModelType> {
  data(): AppModelType;
}

export interface QuerySnapshot<AppModelType = DocumentData> {
  readonly docs: readonly QueryDocumentSnapshot<AppModelType>[];
  readonly empty: boolean;
  readonly metadata: SnapshotMetadata;
  readonly query: Query<AppModelType>;
  readonly size: number;
  forEach(
    callback: (
      result: QueryDocumentSnapshot<AppModelType>,
      index: number,
      array: readonly QueryDocumentSnapshot<AppModelType>[],
    ) => void,
    thisArg?: unknown,
  ): void;
}

export type Unsubscribe = () => void;

export interface WriteBatch {
  commit(): Promise<void>;
  delete<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): WriteBatch;
  set<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    data: AppModelType,
    options?: SetOptions,
  ): WriteBatch;
  update<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    data: DocumentData,
  ): WriteBatch;
}

export interface Transaction {
  delete<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): Transaction;
  get<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): Promise<DocumentSnapshot<AppModelType>>;
  get<AppModelType = DocumentData>(query: Query<AppModelType>): Promise<QuerySnapshot<AppModelType>>;
  set<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    data: AppModelType,
    options?: SetOptions,
  ): Transaction;
  update<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    data: DocumentData,
  ): Transaction;
}

export interface SnapshotObserver<SnapshotType> {
  next?: (snapshot: SnapshotType) => void;
  error?: (error: FirestoreError) => void;
  complete?: () => void;
}

export interface SetOptions {
  merge?: boolean;
  mergeFields?: string[];
}

export interface TransactionOptions {
  maxAttempts?: number;
}

export class FirestoreError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(code: string, message: string, status: number) {
    super(message);
    this.name = "FirestoreError";
    this.code = code;
    this.status = status;
  }
}

class FirestoreImpl implements Firestore {
  readonly app: FirebaseApp;
  readonly databaseId: string;
  #settings: FirestoreSettings;
  #terminated = false;
  #emulatorOptions: FirestoreEmulatorOptions | undefined;
  #terminationHooks = new Set<() => void>();

  constructor(app: FirebaseApp, databaseId: string, settings?: FirestoreSettings) {
    this.app = app;
    this.databaseId = databaseId;
    this.#settings = normalizeSettings(settings);
  }

  get settings(): Readonly<FirestoreSettings> {
    return Object.freeze({ ...this.#settings });
  }

  get terminated(): boolean {
    return this.#terminated;
  }

  get emulatorOptions(): FirestoreEmulatorOptions | undefined {
    return this.#emulatorOptions;
  }

  updateSettings(settings: FirestoreSettings, emulatorOptions?: FirestoreEmulatorOptions): void {
    this.#settings = { ...this.#settings, ...settings };
    this.#emulatorOptions = emulatorOptions;
  }

  addTerminationHook(hook: () => void): void {
    this.#terminationHooks.add(hook);
  }

  removeTerminationHook(hook: () => void): void {
    this.#terminationHooks.delete(hook);
  }

  terminate(): void {
    for (const hook of this.#terminationHooks) {
      hook();
    }
    this.#terminationHooks.clear();
    this.#terminated = true;
  }
}

class DocumentReferenceImpl<AppModelType = DocumentData>
  implements DocumentReference<AppModelType>
{
  readonly converter: FirestoreDataConverter<AppModelType> | null;
  readonly firestore: Firestore;
  readonly id: string;
  readonly parent: CollectionReference<AppModelType>;
  readonly path: string;
  readonly type = "document" as const;

  constructor(
    firestore: Firestore,
    pathSegments: readonly string[],
    parent: CollectionReference<AppModelType>,
    converter: FirestoreDataConverter<AppModelType> | null = null,
  ) {
    this.firestore = firestore;
    this.path = pathSegments.join("/");
    this.id = pathSegments.at(-1) ?? "";
    this.parent = parent;
    this.converter = converter;
  }

  withConverter(converter: null): DocumentReference<DocumentData>;
  withConverter<NewAppModelType>(
    converter: FirestoreDataConverter<NewAppModelType>,
  ): DocumentReference<NewAppModelType>;
  withConverter(
    converter: FirestoreDataConverter<unknown> | null,
  ): DocumentReference<unknown> {
    return buildDocumentReference(
      this.firestore,
      this.path.split("/"),
      converter as FirestoreDataConverter<unknown> | null,
    );
  }
}

class CollectionReferenceImpl<AppModelType = DocumentData>
  implements CollectionReference<AppModelType>
{
  readonly converter: FirestoreDataConverter<AppModelType> | null;
  readonly firestore: Firestore;
  readonly id: string;
  readonly parent: DocumentReference<AppModelType> | null;
  readonly path: string;
  readonly type = "collection" as const;

  constructor(
    firestore: Firestore,
    pathSegments: readonly string[],
    parent: DocumentReference<AppModelType> | null,
    converter: FirestoreDataConverter<AppModelType> | null = null,
  ) {
    this.firestore = firestore;
    this.path = pathSegments.join("/");
    this.id = pathSegments.at(-1) ?? "";
    this.parent = parent;
    this.converter = converter;
  }

  withConverter(converter: null): CollectionReference<DocumentData>;
  withConverter<NewAppModelType>(
    converter: FirestoreDataConverter<NewAppModelType>,
  ): CollectionReference<NewAppModelType>;
  withConverter(
    converter: FirestoreDataConverter<unknown> | null,
  ): CollectionReference<unknown> {
    return buildCollectionReference(
      this.firestore,
      this.path.split("/"),
      converter as FirestoreDataConverter<unknown> | null,
    );
  }
}

class CollectionGroupImpl<AppModelType = DocumentData>
  implements CollectionGroup<AppModelType>
{
  readonly converter: FirestoreDataConverter<AppModelType> | null;
  readonly firestore: Firestore;
  readonly id: string;
  readonly type = "collectionGroup" as const;

  constructor(
    firestore: Firestore,
    collectionId: string,
    converter: FirestoreDataConverter<AppModelType> | null = null,
  ) {
    this.firestore = firestore;
    this.id = collectionId;
    this.converter = converter;
  }

  withConverter(converter: null): CollectionGroup<DocumentData>;
  withConverter<NewAppModelType>(
    converter: FirestoreDataConverter<NewAppModelType>,
  ): CollectionGroup<NewAppModelType>;
  withConverter(
    converter: FirestoreDataConverter<unknown> | null,
  ): CollectionGroup<unknown> {
    return new CollectionGroupImpl(
      this.firestore,
      this.id,
      converter as FirestoreDataConverter<unknown> | null,
    );
  }
}

class QueryImpl<AppModelType = DocumentData> implements Query<AppModelType> {
  readonly converter: FirestoreDataConverter<AppModelType> | null;
  readonly firestore: Firestore;
  readonly source: QuerySource<AppModelType>;
  readonly structuredQuery: Readonly<StructuredQueryShape>;
  readonly type = "query" as const;

  constructor(source: QuerySource<AppModelType>, structuredQuery: StructuredQueryShape) {
    this.firestore = source.firestore;
    this.source = source;
    this.structuredQuery = structuredQuery;
    this.converter = source.converter;
  }

  withConverter(converter: null): Query<DocumentData>;
  withConverter<NewAppModelType>(
    converter: FirestoreDataConverter<NewAppModelType>,
  ): Query<NewAppModelType>;
  withConverter(
    converter: FirestoreDataConverter<unknown> | null,
  ): Query<unknown> {
    const source =
      converter === null
        ? this.source.withConverter(null)
        : (
            this.source as unknown as {
              withConverter(
                converter: FirestoreDataConverter<unknown>,
              ): QuerySource<unknown>;
            }
          ).withConverter(converter);
    return new QueryImpl(
      source as QuerySource<unknown>,
      cloneStructuredQueryShape(this.structuredQuery),
    );
  }
}

class DocumentSnapshotImpl<AppModelType = DocumentData>
  implements DocumentSnapshot<AppModelType>
{
  readonly id: string;
  readonly metadata: SnapshotMetadata;
  readonly ref: DocumentReference<AppModelType>;
  readonly #documentData: DocumentData | undefined;

  constructor(
    ref: DocumentReference<AppModelType>,
    documentData: DocumentData | undefined,
    metadata: SnapshotMetadata = DEFAULT_SNAPSHOT_METADATA,
  ) {
    this.id = ref.id;
    this.ref = ref;
    this.metadata = metadata;
    this.#documentData = documentData;
  }

  exists(): boolean {
    return this.#documentData !== undefined;
  }

  data(): AppModelType | undefined {
    if (this.#documentData === undefined) {
      return undefined;
    }
    if (!this.ref.converter) {
      return this.#documentData as AppModelType;
    }
    const rawRef = buildDocumentReference<DocumentData>(
      this.ref.firestore,
      this.ref.path.split("/"),
    );
    const rawSnapshot = new QueryDocumentSnapshotImpl(
      rawRef,
      this.#documentData,
      this.metadata,
    );
    return this.ref.converter.fromFirestore(rawSnapshot);
  }

  get(fieldPath: string): unknown {
    if (this.#documentData === undefined) {
      return undefined;
    }
    return readValueAtFieldPath(
      this.#documentData as DocumentData,
      splitFieldPath(fieldPath),
    );
  }
}

class QueryDocumentSnapshotImpl<AppModelType = DocumentData>
  extends DocumentSnapshotImpl<AppModelType>
  implements QueryDocumentSnapshot<AppModelType>
{
  override data(): AppModelType {
    const value = super.data();
    if (value === undefined) {
      throw new Error("QueryDocumentSnapshot data must be present.");
    }
    return value;
  }
}

class QuerySnapshotImpl<AppModelType = DocumentData>
  implements QuerySnapshot<AppModelType>
{
  readonly docs: readonly QueryDocumentSnapshot<AppModelType>[];
  readonly empty: boolean;
  readonly metadata: SnapshotMetadata;
  readonly query: Query<AppModelType>;
  readonly size: number;

  constructor(
    query: Query<AppModelType>,
    docs: readonly QueryDocumentSnapshot<AppModelType>[],
    metadata: SnapshotMetadata = DEFAULT_SNAPSHOT_METADATA,
  ) {
    this.docs = docs;
    this.empty = docs.length === 0;
    this.metadata = metadata;
    this.query = query;
    this.size = docs.length;
  }

  forEach(
    callback: (
      result: QueryDocumentSnapshot<AppModelType>,
      index: number,
      array: readonly QueryDocumentSnapshot<AppModelType>[],
    ) => void,
    thisArg?: unknown,
  ): void {
    this.docs.forEach(callback, thisArg);
  }
}

class WriteBatchImpl implements WriteBatch {
  readonly #firestore: Firestore;
  #committed = false;
  #writes: Record<string, unknown>[] = [];

  constructor(firestore: Firestore) {
    this.#firestore = firestore;
  }

  set<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    data: AppModelType,
    options?: SetOptions,
  ): WriteBatch {
    this.#assertMutable();
    this.#assertSameFirestore(reference.firestore);
    const documentData = toFirestoreDocumentData(
      reference.converter,
      data,
      "WriteBatch.set data",
    );
    this.#writes.push(
      buildSetWriteInternal(
        reference,
        documentData,
        options,
        FIRESTORE_WRITE_DEPENDENCIES,
      ),
    );
    return this;
  }

  update<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    data: DocumentData,
  ): WriteBatch {
    this.#assertMutable();
    this.#assertSameFirestore(reference.firestore);
    const documentData = assertDocumentData(data, "WriteBatch.update data");
    this.#writes.push(
      buildUpdateWriteInternal(
        reference,
        documentData,
        FIRESTORE_WRITE_DEPENDENCIES,
      ),
    );
    return this;
  }

  delete<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): WriteBatch {
    this.#assertMutable();
    this.#assertSameFirestore(reference.firestore);
    this.#writes.push(
      buildDeleteWriteInternal(reference, FIRESTORE_WRITE_DEPENDENCIES),
    );
    return this;
  }

  async commit(): Promise<void> {
    this.#assertMutable();
    this.#committed = true;
    await commitWrites(this.#firestore, this.#writes);
  }

  #assertMutable(): void {
    if (this.#committed) {
      throw new Error("WriteBatch cannot be used after commit().");
    }
  }

  #assertSameFirestore(firestore: Firestore): void {
    if (firestore !== this.#firestore) {
      throw new Error("WriteBatch references must all belong to the same Firestore.");
    }
  }
}

class TransactionImpl implements Transaction {
  readonly #firestore: Firestore;
  readonly #transaction: Uint8Array;
  #closed = false;
  #writes: Record<string, unknown>[] = [];

  constructor(firestore: Firestore, transaction: Uint8Array) {
    this.#firestore = firestore;
    this.#transaction = transaction;
  }

  async get<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): Promise<DocumentSnapshot<AppModelType>>;
  async get<AppModelType = DocumentData>(
    query: Query<AppModelType>,
  ): Promise<QuerySnapshot<AppModelType>>;
  async get<AppModelType = DocumentData>(
    source: DocumentReference<AppModelType> | Query<AppModelType>,
  ): Promise<DocumentSnapshot<AppModelType> | QuerySnapshot<AppModelType>> {
    this.#assertOpen();
    if (this.#writes.length > 0) {
      throw new Error("Firestore transactions require all reads to happen before writes.");
    }
    this.#assertSameFirestore(source.firestore);
    if (isQuery(source)) {
      return runQueryDocuments(source as Query<AppModelType>, {
        transaction: this.#transaction,
      });
    }
    return batchGetDocument(source as DocumentReference<AppModelType>, {
      transaction: this.#transaction,
    });
  }

  set<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    data: AppModelType,
    options?: SetOptions,
  ): Transaction {
    this.#assertOpen();
    this.#assertSameFirestore(reference.firestore);
    const documentData = toFirestoreDocumentData(
      reference.converter,
      data,
      "Transaction.set data",
    );
    this.#writes.push(
      buildSetWriteInternal(
        reference,
        documentData,
        options,
        FIRESTORE_WRITE_DEPENDENCIES,
      ),
    );
    return this;
  }

  update<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    data: DocumentData,
  ): Transaction {
    this.#assertOpen();
    this.#assertSameFirestore(reference.firestore);
    const documentData = assertDocumentData(data, "Transaction.update data");
    this.#writes.push(
      buildUpdateWriteInternal(
        reference,
        documentData,
        FIRESTORE_WRITE_DEPENDENCIES,
      ),
    );
    return this;
  }

  delete<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): Transaction {
    this.#assertOpen();
    this.#assertSameFirestore(reference.firestore);
    this.#writes.push(
      buildDeleteWriteInternal(reference, FIRESTORE_WRITE_DEPENDENCIES),
    );
    return this;
  }

  get hasWrites(): boolean {
    return this.#writes.length > 0;
  }

  async commit(): Promise<void> {
    this.#assertOpen();
    this.#closed = true;
    await commitWrites(this.#firestore, this.#writes, {
      transaction: this.#transaction,
    });
  }

  async rollback(): Promise<void> {
    this.#assertOpen();
    this.#closed = true;
    await rollbackFirestoreTransaction(this.#firestore, this.#transaction);
  }

  async rollbackSilently(): Promise<void> {
    if (this.#closed) {
      return;
    }
    this.#closed = true;
    try {
      await rollbackFirestoreTransaction(this.#firestore, this.#transaction);
    } catch {
      // Preserve the original transaction error; server-owned sessions expire.
    }
  }

  #assertOpen(): void {
    if (this.#closed) {
      throw new Error("Transaction cannot be used after it has finished.");
    }
  }

  #assertSameFirestore(firestore: Firestore): void {
    if (firestore !== this.#firestore) {
      throw new Error("Transaction references must all belong to the same Firestore.");
    }
  }
}

const registries = new WeakMap<FirebaseApp, Map<string, FirestoreImpl>>();

function normalizedDatabaseId(databaseId?: string): string {
  const candidate = databaseId?.trim() ?? DEFAULT_DATABASE_ID;
  if (candidate.length === 0) {
    throw new Error("Firestore database ID must not be empty.");
  }
  return candidate;
}

function normalizeSettings(settings?: FirestoreSettings): FirestoreSettings {
  return {
    host: settings?.host ?? DEFAULT_HOST,
    ssl: settings?.ssl ?? true,
    ignoreUndefinedProperties: settings?.ignoreUndefinedProperties ?? false,
    experimentalForceLongPolling: settings?.experimentalForceLongPolling ?? false,
    experimentalAutoDetectLongPolling:
      settings?.experimentalAutoDetectLongPolling ?? false,
    useFetchStreams: settings?.useFetchStreams ?? true,
    experimentalFetch: settings?.experimentalFetch,
    experimentalWebSocketFactory: settings?.experimentalWebSocketFactory,
    experimentalHeaders: settings?.experimentalHeaders,
    experimentalAuthToken: settings?.experimentalAuthToken,
    experimentalUnaryTransport: settings?.experimentalUnaryTransport ?? "rest",
  };
}

function registryFor(app: FirebaseApp): Map<string, FirestoreImpl> {
  let registry = registries.get(app);
  if (!registry) {
    registry = new Map<string, FirestoreImpl>();
    registries.set(app, registry);
  }
  return registry;
}

const DOCUMENT_ID_FIELD_PATH: DocumentIdFieldPath = Object.freeze({
  type: "documentIdFieldPath",
});

function firestoreImpl(firestore: Firestore): FirestoreImpl {
  return firestore as FirestoreImpl;
}

function buildCollectionReference<AppModelType = DocumentData>(
  firestore: Firestore,
  pathSegments: readonly string[],
  converter: FirestoreDataConverter<AppModelType> | null = null,
): CollectionReference<AppModelType> {
  assertCollectionPath(pathSegments);
  const parent =
    pathSegments.length <= 1
      ? null
      : (buildDocumentReference<DocumentData>(
          firestore,
          pathSegments.slice(0, -1),
        ) as DocumentReference<AppModelType>);
  return new CollectionReferenceImpl(firestore, pathSegments, parent, converter);
}

function buildDocumentReference<AppModelType = DocumentData>(
  firestore: Firestore,
  pathSegments: readonly string[],
  converter: FirestoreDataConverter<AppModelType> | null = null,
): DocumentReference<AppModelType> {
  assertDocumentPath(pathSegments);
  const parent = buildCollectionReference<AppModelType>(
    firestore,
    pathSegments.slice(0, -1),
    converter,
  );
  return new DocumentReferenceImpl(firestore, pathSegments, parent, converter);
}

function documentResourceName<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
): string {
  return `${databaseResourceName(reference.firestore)}/documents/${reference.path}`;
}

function parseDocumentReferenceFromName<AppModelType = DocumentData>(
  firestore: Firestore,
  documentName: string,
  converter: FirestoreDataConverter<AppModelType> | null = null,
): DocumentReference<AppModelType> {
  const prefix = `${databaseResourceName(firestore)}/documents/`;
  if (!documentName.startsWith(prefix)) {
    throw new Error(
      `Firestore document name "${documentName}" did not match database "${databaseResourceName(firestore)}".`,
    );
  }
  const relativePath = documentName.slice(prefix.length);
  return buildDocumentReference<AppModelType>(
    firestore,
    relativePath.split("/"),
    converter,
  );
}

function normalizeQuerySource<AppModelType = DocumentData>(
  source: QuerySource<AppModelType> | Query<AppModelType>,
): Query<AppModelType> {
  if (isQuery(source)) {
    return source;
  }
  const querySource = source as QuerySource<AppModelType>;
  return new QueryImpl(querySource, baseStructuredQuery(querySource));
}

function maybeMockUserToken(firestore: Firestore): string | null {
  const mockUserToken = firestoreImpl(firestore).emulatorOptions?.mockUserToken;
  if (mockUserToken === undefined) {
    return null;
  }
  if (typeof mockUserToken === "string") {
    return mockUserToken;
  }
  return JSON.stringify(mockUserToken);
}

const FIRESTORE_AUTH_DEPENDENCIES: FirestoreAuthDependencies = {
  mockUserToken: maybeMockUserToken,
};

const FIRESTORE_WRITE_DEPENDENCIES: FirestoreWriteDependencies = {
  documentResourceName,
};

const FIRESTORE_UNARY_DEPENDENCIES: FirestoreUnaryDependencies = {
  canRefreshAuthToken,
  resolveAuthToken: (firestore, forceRefresh) =>
    resolveAuthToken(firestore, forceRefresh, FIRESTORE_AUTH_DEPENDENCIES),
  createGrpcWebContext: (firestore) =>
    grpcWebContext(firestore, FIRESTORE_AUTH_DEPENDENCIES),
  createFirestoreError: (code, message, status) =>
    new FirestoreError(code, message, status),
  databaseBaseUrl,
  databaseResourceName,
  documentResourceName,
  queryParentResourceName,
  decodeDocumentFields,
  buildDocumentSnapshot: (reference, documentData) =>
    new DocumentSnapshotImpl(reference, documentData),
  buildQuerySnapshot: (query, documents) => {
    const docs = documents.map(({ name, documentData }) => {
      const ref = parseDocumentReferenceFromName(
        query.firestore,
        name,
        query.converter,
      );
      return new QueryDocumentSnapshotImpl(ref, documentData);
    });
    return new QuerySnapshotImpl(query, docs);
  },
};

const FIRESTORE_WATCH_SNAPSHOT_DEPENDENCIES: FirestoreWatchSnapshotDependencies = {
  buildDocumentSnapshot: (reference, documentData) =>
    new DocumentSnapshotImpl(reference, documentData),
  buildQuerySnapshot: (query, documents) => {
    const docs = documents.map(({ name, documentData }) => {
      const ref = parseDocumentReferenceFromName(
        query.firestore,
        name,
        query.converter,
      );
      return new QueryDocumentSnapshotImpl(ref, documentData);
    });
    return new QuerySnapshotImpl(query, docs);
  },
  deepEqualValue,
  isPlainObject,
  readValueAtFieldPath,
  splitFieldPath,
};

function normalizeTransactionAttempts(options?: TransactionOptions): number {
  const maxAttempts = options?.maxAttempts ?? 5;
  if (!Number.isSafeInteger(maxAttempts) || maxAttempts <= 0) {
    throw new Error("Firestore transaction maxAttempts must be a positive integer.");
  }
  return maxAttempts;
}

function isRetryableTransactionError(error: unknown): error is FirestoreError {
  return error instanceof FirestoreError && error.code === "ABORTED";
}

function queryParentResourceName<AppModelType = DocumentData>(
  query: Query<AppModelType>,
): string {
  if (isCollectionReference(query.source) && query.source.parent) {
    return documentResourceName(query.source.parent);
  }
  return `${databaseResourceName(query.firestore)}/documents`;
}

async function commitWrites(
  firestore: Firestore,
  writes: readonly Record<string, unknown>[],
  options?: {
    transaction?: Uint8Array;
  },
): Promise<void> {
  return commitWritesInternal(
    firestore,
    writes,
    options,
    FIRESTORE_UNARY_DEPENDENCIES,
  );
}

async function batchGetDocument<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  options?: {
    transaction?: Uint8Array;
  },
): Promise<DocumentSnapshot<AppModelType>> {
  return batchGetDocumentInternal(
    reference,
    options,
    FIRESTORE_UNARY_DEPENDENCIES,
  );
}

async function beginFirestoreTransaction(firestore: Firestore): Promise<Uint8Array> {
  return beginFirestoreTransactionInternal(
    firestore,
    FIRESTORE_UNARY_DEPENDENCIES,
  );
}

async function rollbackFirestoreTransaction(
  firestore: Firestore,
  transaction: Uint8Array,
): Promise<void> {
  return rollbackFirestoreTransactionInternal(
    firestore,
    transaction,
    FIRESTORE_UNARY_DEPENDENCIES,
  );
}

async function runQueryDocuments<AppModelType = DocumentData>(
  query: Query<AppModelType>,
  options?: {
    transaction?: Uint8Array;
  },
): Promise<QuerySnapshot<AppModelType>> {
  return runQueryDocumentsInternal(
    query,
    options,
    FIRESTORE_UNARY_DEPENDENCIES,
  );
}

export function initializeFirestore(
  app: FirebaseApp,
  settings?: FirestoreSettings,
  databaseId?: string,
): Firestore {
  const targetDatabaseId = normalizedDatabaseId(databaseId);
  const registry = registryFor(app);
  if (registry.has(targetDatabaseId)) {
    throw new Error(
      `Firestore database "${targetDatabaseId}" is already initialized for app "${app.name}".`,
    );
  }
  const firestore = new FirestoreImpl(app, targetDatabaseId, settings);
  registry.set(targetDatabaseId, firestore);
  return firestore;
}

export function getFirestore(
  appOrDatabaseId?: FirebaseApp | string,
  databaseId?: string,
): Firestore {
  const resolved = resolveAppAndDatabase(appOrDatabaseId, databaseId);
  const registry = registryFor(resolved.app);
  const existing = registry.get(resolved.databaseId);
  if (existing) {
    return existing;
  }
  const firestore = new FirestoreImpl(resolved.app, resolved.databaseId);
  registry.set(resolved.databaseId, firestore);
  return firestore;
}

export function collection<AppModelType = DocumentData>(
  source: Firestore | DocumentReference<AppModelType> | CollectionReference<AppModelType>,
  path: string,
  ...pathSegments: string[]
): CollectionReference<AppModelType> {
  const fullPath = composePath(source, path, pathSegments);
  return buildCollectionReference(firestoreFromSource(source), fullPath);
}

export function doc<AppModelType = DocumentData>(
  source: Firestore | DocumentReference<AppModelType> | CollectionReference<AppModelType>,
  path: string,
  ...pathSegments: string[]
): DocumentReference<AppModelType> {
  const fullPath = composePath(source, path, pathSegments);
  const converter = isCollectionReference(source) ? source.converter : null;
  return buildDocumentReference<AppModelType>(
    firestoreFromSource(source),
    fullPath,
    converter as FirestoreDataConverter<AppModelType> | null,
  );
}

export function collectionGroup<AppModelType = DocumentData>(
  firestore: Firestore,
  collectionId: string,
): CollectionGroup<AppModelType> {
  return new CollectionGroupImpl(firestore, assertCollectionId(collectionId));
}

export function documentId(): DocumentIdFieldPath {
  return DOCUMENT_ID_FIELD_PATH;
}

export function where(
  fieldPath: string | DocumentIdFieldPath,
  op: WhereFilterOp,
  value: unknown,
): QueryConstraint {
  if (
    (op === "in" || op === "array-contains-any" || op === "not-in") &&
    !Array.isArray(value)
  ) {
    throw new Error(`Firestore ${op} filters require an array comparison value.`);
  }
  return {
    fieldPath: assertQueryFieldPath(fieldPath, "where"),
    op,
    type: "where",
    value,
  };
}

export function orderBy(
  fieldPath: string | DocumentIdFieldPath,
  direction: OrderByDirection = "asc",
): QueryConstraint {
  return {
    direction,
    fieldPath: assertQueryFieldPath(fieldPath, "orderBy"),
    type: "orderBy",
  };
}

export function limit(count: number): QueryConstraint {
  if (!Number.isSafeInteger(count) || count <= 0) {
    throw new Error("Firestore limit() requires a positive integer.");
  }
  return {
    count,
    type: "limit",
  };
}

export function startAt(...values: unknown[]): QueryConstraint {
  return {
    before: true,
    kind: "startAt",
    type: "cursor",
    values: assertCursorValues(values, "startAt()"),
  };
}

export function startAfter(...values: unknown[]): QueryConstraint {
  return {
    before: false,
    kind: "startAt",
    type: "cursor",
    values: assertCursorValues(values, "startAfter()"),
  };
}

export function endAt(...values: unknown[]): QueryConstraint {
  return {
    before: false,
    kind: "endAt",
    type: "cursor",
    values: assertCursorValues(values, "endAt()"),
  };
}

export function endBefore(...values: unknown[]): QueryConstraint {
  return {
    before: true,
    kind: "endAt",
    type: "cursor",
    values: assertCursorValues(values, "endBefore()"),
  };
}

export function query<AppModelType = DocumentData>(
  source:
    | QuerySource<AppModelType>
    | Query<AppModelType>,
  ...constraints: QueryConstraint[]
): Query<AppModelType> {
  let querySource: QuerySource<AppModelType>;
  let initialShape: StructuredQueryShape;
  if (isQuery(source)) {
    querySource = source.source;
    initialShape = cloneStructuredQueryShape(source.structuredQuery);
  } else {
    querySource = source as QuerySource<AppModelType>;
    initialShape = baseStructuredQuery(querySource);
  }
  const structuredQuery = constraints.reduce(applyQueryConstraint, initialShape);
  return new QueryImpl(querySource, structuredQuery);
}

export function refEqual<
  LeftType = DocumentData,
  RightType = DocumentData,
>(
  left:
    | DocumentReference<LeftType>
    | CollectionReference<LeftType>
    | CollectionGroup<LeftType>,
  right:
    | DocumentReference<RightType>
    | CollectionReference<RightType>
    | CollectionGroup<RightType>,
): boolean {
  if (!firestoreIdentityEqual(left.firestore, right.firestore) || left.type !== right.type) {
    return false;
  }
  if (isCollectionGroup(left) && isCollectionGroup(right)) {
    return left.id === right.id;
  }
  if (isDocumentReference(left) && isDocumentReference(right)) {
    return left.path === right.path;
  }
  if (isCollectionReference(left) && isCollectionReference(right)) {
    return left.path === right.path;
  }
  return false;
}

export function queryEqual<LeftType = DocumentData, RightType = DocumentData>(
  left: Query<LeftType>,
  right: Query<RightType>,
): boolean {
  return (
    refEqual(left.source, right.source) &&
    deepEqualValue(left.structuredQuery, right.structuredQuery)
  );
}

export function snapshotEqual<LeftType = DocumentData, RightType = DocumentData>(
  left:
    | DocumentSnapshot<LeftType>
    | QuerySnapshot<LeftType>,
  right:
    | DocumentSnapshot<RightType>
    | QuerySnapshot<RightType>,
): boolean {
  if (isQuerySnapshot(left) || isQuerySnapshot(right)) {
    if (!isQuerySnapshot(left) || !isQuerySnapshot(right)) {
      return false;
    }
    return (
      queryEqual(left.query, right.query) &&
      left.size === right.size &&
      left.empty === right.empty &&
      left.docs.every((snapshot, index) => snapshotEqual(snapshot, right.docs[index])) &&
      left.metadata.fromCache === right.metadata.fromCache &&
      left.metadata.hasPendingWrites === right.metadata.hasPendingWrites
    );
  }
  if (!isDocumentSnapshot(left) || !isDocumentSnapshot(right)) {
    return false;
  }
  return (
    refEqual(left.ref, right.ref) &&
    left.exists() === right.exists() &&
    deepEqualValue(left.data(), right.data()) &&
    left.metadata.fromCache === right.metadata.fromCache &&
    left.metadata.hasPendingWrites === right.metadata.hasPendingWrites
  );
}

export async function getDocs<AppModelType = DocumentData>(
  source: QuerySource<AppModelType> | Query<AppModelType>,
): Promise<QuerySnapshot<AppModelType>> {
  return runQueryDocuments(normalizeQuerySource(source));
}

export async function getDoc<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
): Promise<DocumentSnapshot<AppModelType>> {
  const snapshot = await batchGetDocument(reference);
  return snapshot as DocumentSnapshot<AppModelType>;
}

export async function setDoc<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  data: AppModelType,
  options?: SetOptions,
): Promise<void> {
  const documentData = toFirestoreDocumentData(reference.converter, data, "setDoc data");
  await commitWrites(reference.firestore, [
    buildSetWriteInternal(
      reference,
      documentData,
      options,
      FIRESTORE_WRITE_DEPENDENCIES,
    ),
  ]);
}

export async function updateDoc<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  data: DocumentData,
): Promise<void> {
  const documentData = assertDocumentData(data, "updateDoc data");
  await commitWrites(reference.firestore, [
    buildUpdateWriteInternal(
      reference,
      documentData,
      FIRESTORE_WRITE_DEPENDENCIES,
    ),
  ]);
}

export async function deleteDoc<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
): Promise<void> {
  await commitWrites(reference.firestore, [
    buildDeleteWriteInternal(reference, FIRESTORE_WRITE_DEPENDENCIES),
  ]);
}

export async function addDoc<AppModelType = DocumentData>(
  reference: CollectionReference<AppModelType>,
  data: AppModelType,
): Promise<DocumentReference<AppModelType>> {
  const documentData = toFirestoreDocumentData(reference.converter, data, "addDoc data");
  const createdReference = doc(reference, createAutoId());
  await commitWrites(reference.firestore, [
    buildCreateWriteInternal(
      createdReference,
      documentData,
      FIRESTORE_WRITE_DEPENDENCIES,
    ),
  ]);
  return createdReference;
}

export function writeBatch(firestore: Firestore): WriteBatch {
  return new WriteBatchImpl(firestore);
}

export async function runTransaction<T>(
  firestore: Firestore,
  updateFunction: (transaction: Transaction) => Promise<T> | T,
  options?: TransactionOptions,
): Promise<T> {
  const maxAttempts = normalizeTransactionAttempts(options);

  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    const transaction = new TransactionImpl(
      firestore,
      await beginFirestoreTransaction(firestore),
    );
    try {
      const result = await updateFunction(transaction);
      if (transaction.hasWrites) {
        await transaction.commit();
      } else {
        await transaction.rollback();
      }
      return result;
    } catch (error) {
      await transaction.rollbackSilently();
      if (isRetryableTransactionError(error) && attempt < maxAttempts) {
        continue;
      }
      throw error;
    }
  }

  throw new Error("Firestore transaction attempts exhausted.");
}

function listenWebSocketUrl(firestore: Firestore): string {
  const protocol = firestore.settings.ssl ? "wss" : "ws";
  return `${protocol}://${firestore.settings.host}/google.firestore.v1.Firestore/Listen`;
}

export function onSnapshot<Source extends SnapshotListenSource>(
  source: Source,
  observerOrNext:
    | SnapshotObserver<SnapshotForSource<Source>>
    | ((snapshot: SnapshotForSource<Source>) => void),
  onError?: (error: FirestoreError) => void,
  onCompletion?: () => void,
): Unsubscribe {
  return onSnapshotInternal(source, observerOrNext, onError, onCompletion, {
    databaseResourceName,
    documentResourceName,
    queryParentResourceName,
    encodeStructuredQueryForTransport,
    canRefreshAuthToken,
    listenWebSocketUrl,
    resolveListenWebSocketSubprotocols: (firestore, forceRefresh) =>
      resolveListenWebSocketSubprotocols(
        firestore,
        forceRefresh,
        FIRESTORE_AUTH_DEPENDENCIES,
      ),
    firestoreImpl,
    isDocumentReference,
    normalizeQuerySource,
    isPlainObject,
    decodeDocumentFields,
    buildDocumentSnapshot: (reference, documentData) =>
      buildWatchDocumentSnapshotInternal(
        reference,
        documentData,
        FIRESTORE_WATCH_SNAPSHOT_DEPENDENCIES,
      ),
    buildQuerySnapshot: (query, documents) =>
      buildWatchQuerySnapshotInternal(
        query,
        documents,
        FIRESTORE_WATCH_SNAPSHOT_DEPENDENCIES,
      ),
    createFirestoreError: (code, message, status) =>
      new FirestoreError(code, message, status),
  });
}

export function connectFirestoreEmulator(
  firestore: Firestore,
  host: string,
  port: number,
  options?: FirestoreEmulatorOptions,
): void {
  const target = firestoreImpl(firestore);
  target.updateSettings(
    {
      host: `${host}:${port}`,
      ssl: false,
      useFetchStreams: false,
    },
    options,
  );
}

export async function terminate(firestore: Firestore): Promise<void> {
  const target = firestoreImpl(firestore);
  target.terminate();
}
