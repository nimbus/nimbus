import type { FirebaseApp } from "../app";
import type {
  CollectionGroup,
  CollectionReference,
  DocumentData,
  DocumentReference,
  DocumentSnapshot,
  Firestore,
  FirestoreDataConverter,
  FirestoreEmulatorOptions,
  FirestoreSettings,
  Query,
  QueryDocumentSnapshot,
  QuerySnapshot,
  SnapshotMetadata,
} from "../firestore";
import {
  readValueAtFieldPath,
  splitFieldPath,
} from "./document-data";
import {
  assertCollectionId,
  assertCollectionPath,
  assertDocumentPath,
  baseStructuredQuery,
  cloneStructuredQueryShape,
  databaseResourceName,
  isCollectionReference,
  isQuery,
  type QuerySource,
  type StructuredQueryShape,
} from "./firestore-helpers";

export const DEFAULT_DATABASE_ID = "(default)";
const DEFAULT_HOST = "firestore.googleapis.com";

export const DEFAULT_SNAPSHOT_METADATA = Object.freeze({
  fromCache: false,
  hasPendingWrites: false,
});

export function normalizedDatabaseId(databaseId?: string): string {
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

export class FirestoreImpl implements Firestore {
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

export class DocumentReferenceImpl<AppModelType = DocumentData>
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

export class CollectionReferenceImpl<AppModelType = DocumentData>
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

export class CollectionGroupImpl<AppModelType = DocumentData>
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

export class QueryImpl<AppModelType = DocumentData> implements Query<AppModelType> {
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

export class DocumentSnapshotImpl<AppModelType = DocumentData>
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

export class QueryDocumentSnapshotImpl<AppModelType = DocumentData>
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

export class QuerySnapshotImpl<AppModelType = DocumentData>
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

export function firestoreImpl(firestore: Firestore): FirestoreImpl {
  return firestore as FirestoreImpl;
}

export function buildCollectionReference<AppModelType = DocumentData>(
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

export function buildDocumentReference<AppModelType = DocumentData>(
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

export function documentResourceName<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
): string {
  return `${databaseResourceName(reference.firestore)}/documents/${reference.path}`;
}

export function parseDocumentReferenceFromName<AppModelType = DocumentData>(
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

export function normalizeQuerySource<AppModelType = DocumentData>(
  source: QuerySource<AppModelType> | Query<AppModelType>,
): Query<AppModelType> {
  if (isQuery(source)) {
    return source;
  }
  const querySource = source as QuerySource<AppModelType>;
  return new QueryImpl(querySource, baseStructuredQuery(querySource));
}

export function buildCollectionGroup<AppModelType = DocumentData>(
  firestore: Firestore,
  collectionId: string,
): CollectionGroup<AppModelType> {
  return new CollectionGroupImpl(firestore, assertCollectionId(collectionId));
}
