import type { FirebaseApp } from "../app";
import { getApp } from "../app";
import { assertDocumentData, isPlainObject } from "./document-data";
import type {
  CollectionGroup,
  CollectionReference,
  DocumentData,
  DocumentIdFieldPath,
  DocumentSnapshot,
  DocumentReference,
  Firestore,
  FirestoreDataConverter,
  FirestoreSettings,
  OrderByDirection,
  Query,
  QueryConstraint,
  QuerySnapshot,
  SnapshotMetadata,
  WhereFilterOp,
} from "../firestore";

const DEFAULT_DATABASE_ID = "(default)";
const DEFAULT_HOST = "firestore.googleapis.com";
const AUTO_ID_ALPHABET =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const AUTO_ID_LENGTH = 20;

export type QuerySource<AppModelType = DocumentData> =
  | CollectionReference<AppModelType>
  | CollectionGroup<AppModelType>;

export interface StructuredQueryCollectionSelector {
  readonly collectionId: string;
  readonly allDescendants?: boolean;
}

export interface StructuredQueryFieldReference {
  readonly fieldPath: string;
}

export interface StructuredQueryFieldFilter {
  readonly field: StructuredQueryFieldReference;
  readonly op: FirestoreStructuredWhereOperator;
  readonly value: unknown;
}

export interface StructuredQueryCompositeFilter {
  readonly op: "AND";
  readonly filters: readonly StructuredQueryFilter[];
}

export type StructuredQueryFilter =
  | { readonly fieldFilter: StructuredQueryFieldFilter }
  | { readonly compositeFilter: StructuredQueryCompositeFilter };

export interface StructuredQueryOrder {
  readonly field: StructuredQueryFieldReference;
  readonly direction: FirestoreStructuredDirection;
}

export interface StructuredQueryCursor {
  readonly before: boolean;
  readonly values: readonly unknown[];
}

export interface WhereQueryConstraint {
  readonly fieldPath: string;
  readonly op: WhereFilterOp;
  readonly type: "where";
  readonly value: unknown;
}

export interface OrderByQueryConstraint {
  readonly direction: OrderByDirection;
  readonly fieldPath: string;
  readonly type: "orderBy";
}

export interface LimitQueryConstraint {
  readonly count: number;
  readonly type: "limit";
}

export interface CursorQueryConstraint {
  readonly before: boolean;
  readonly kind: "startAt" | "endAt";
  readonly type: "cursor";
  readonly values: readonly unknown[];
}

type FirestoreStructuredWhereOperator =
  | "LESS_THAN"
  | "LESS_THAN_OR_EQUAL"
  | "EQUAL"
  | "NOT_EQUAL"
  | "GREATER_THAN"
  | "GREATER_THAN_OR_EQUAL"
  | "ARRAY_CONTAINS"
  | "IN"
  | "ARRAY_CONTAINS_ANY"
  | "NOT_IN";

type FirestoreStructuredDirection = "ASCENDING" | "DESCENDING";

export interface StructuredQueryShape {
  readonly endAt?: StructuredQueryCursor;
  readonly from: readonly StructuredQueryCollectionSelector[];
  readonly limit?: number;
  readonly orderBy?: readonly StructuredQueryOrder[];
  readonly startAt?: StructuredQueryCursor;
  readonly where?: StructuredQueryFilter;
}

const WHERE_OPERATOR_MAP: Readonly<Record<WhereFilterOp, FirestoreStructuredWhereOperator>> = {
  "<": "LESS_THAN",
  "<=": "LESS_THAN_OR_EQUAL",
  "==": "EQUAL",
  "!=": "NOT_EQUAL",
  ">=": "GREATER_THAN_OR_EQUAL",
  ">": "GREATER_THAN",
  "array-contains": "ARRAY_CONTAINS",
  in: "IN",
  "array-contains-any": "ARRAY_CONTAINS_ANY",
  "not-in": "NOT_IN",
};

const ORDER_DIRECTION_MAP: Readonly<Record<OrderByDirection, FirestoreStructuredDirection>> = {
  asc: "ASCENDING",
  desc: "DESCENDING",
};

export function normalizedDatabaseId(databaseId?: string): string {
  const normalized = databaseId?.trim();
  return normalized && normalized.length > 0 ? normalized : DEFAULT_DATABASE_ID;
}

export function normalizeSettings(settings?: FirestoreSettings): FirestoreSettings {
  return {
    experimentalAutoDetectLongPolling: settings?.experimentalAutoDetectLongPolling ?? false,
    experimentalAuthToken: settings?.experimentalAuthToken,
    experimentalFetch: settings?.experimentalFetch,
    experimentalForceLongPolling: settings?.experimentalForceLongPolling ?? false,
    experimentalHeaders: settings?.experimentalHeaders,
    experimentalUnaryTransport: settings?.experimentalUnaryTransport ?? "rest",
    experimentalWebSocketFactory: settings?.experimentalWebSocketFactory,
    host: settings?.host ?? DEFAULT_HOST,
    ignoreUndefinedProperties: settings?.ignoreUndefinedProperties ?? false,
    ssl: settings?.ssl ?? true,
    useFetchStreams: settings?.useFetchStreams ?? !settings?.experimentalForceLongPolling,
  };
}

export function isCollectionReference(value: unknown): value is CollectionReference {
  return (
    typeof value === "object" &&
    value !== null &&
    "type" in value &&
    (value as { type?: unknown }).type === "collection"
  );
}

export function isDocumentReference(value: unknown): value is DocumentReference {
  return (
    typeof value === "object" &&
    value !== null &&
    "type" in value &&
    (value as { type?: unknown }).type === "document"
  );
}

export function isCollectionGroup(value: unknown): value is CollectionGroup {
  return (
    typeof value === "object" &&
    value !== null &&
    "type" in value &&
    (value as { type?: unknown }).type === "collectionGroup"
  );
}

export function isQuery(value: unknown): value is Query {
  return (
    typeof value === "object" &&
    value !== null &&
    "type" in value &&
    (value as { type?: unknown }).type === "query"
  );
}

export function isQuerySnapshot(value: unknown): value is QuerySnapshot {
  return (
    typeof value === "object" &&
    value !== null &&
    "query" in value &&
    "docs" in value &&
    "metadata" in value
  );
}

export function isDocumentSnapshot(value: unknown): value is DocumentSnapshot {
  return (
    typeof value === "object" &&
    value !== null &&
    "ref" in value &&
    "metadata" in value &&
    "data" in value
  );
}

export function isDocumentIdFieldPath(value: unknown): value is DocumentIdFieldPath {
  return typeof value === "object" && value !== null && (value as { type?: unknown }).type === "documentIdFieldPath";
}

export function firestoreFromSource<AppModelType = DocumentData>(
  source: Firestore | DocumentReference<AppModelType> | CollectionReference<AppModelType>,
): Firestore {
  return isDocumentReference(source) || isCollectionReference(source)
    ? source.firestore
    : (source as Firestore);
}

export function firestoreIdentityEqual(left: Firestore, right: Firestore): boolean {
  return left.app.name === right.app.name && left.databaseId === right.databaseId;
}

export function deepEqualValue(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) {
    return true;
  }
  if (Array.isArray(left) && Array.isArray(right)) {
    return (
      left.length === right.length &&
      left.every((value, index) => deepEqualValue(value, right[index]))
    );
  }
  if (isPlainObject(left) && isPlainObject(right)) {
    const leftKeys = Object.keys(left);
    const rightKeys = Object.keys(right);
    return (
      leftKeys.length === rightKeys.length &&
      leftKeys.every(
        (key) =>
          Object.hasOwn(right, key) && deepEqualValue(left[key], right[key]),
      )
    );
  }
  return false;
}

function basePathSegments<AppModelType = DocumentData>(
  source: Firestore | DocumentReference<AppModelType> | CollectionReference<AppModelType>,
): string[] {
  if (isDocumentReference(source) || isCollectionReference(source)) {
    return source.path === "" ? [] : source.path.split("/");
  }
  return [];
}

function splitPathInput(firstSegment: string, additionalSegments: string[]): string[] {
  const allParts = [firstSegment, ...additionalSegments].flatMap((segment) =>
    segment.split("/"),
  );
  if (allParts.length === 0) {
    throw new Error("Firestore path must include at least one segment.");
  }
  if (allParts.some((segment) => segment.length === 0)) {
    throw new Error("Firestore path segments must not be empty.");
  }
  return allParts;
}

export function composePath<AppModelType = DocumentData>(
  source: Firestore | DocumentReference<AppModelType> | CollectionReference<AppModelType>,
  firstSegment: string,
  additionalSegments: string[],
): string[] {
  return [...basePathSegments(source), ...splitPathInput(firstSegment, additionalSegments)];
}

export function assertDocumentPath(pathSegments: readonly string[]): void {
  if (pathSegments.length === 0 || pathSegments.length % 2 !== 0) {
    throw new Error(
      "Document references must resolve to an even number of path segments.",
    );
  }
}

export function assertCollectionPath(pathSegments: readonly string[]): void {
  if (pathSegments.length === 0 || pathSegments.length % 2 === 0) {
    throw new Error(
      "Collection references must resolve to an odd number of path segments.",
    );
  }
}

export function assertCollectionId(collectionId: string): string {
  const normalized = collectionId.trim();
  if (normalized.length === 0) {
    throw new Error("Collection group IDs must not be empty.");
  }
  if (normalized.includes("/")) {
    throw new Error("Collection group IDs must be a single collection segment.");
  }
  return normalized;
}

function cloneQueryValue<T>(value: T): T {
  if (typeof globalThis.structuredClone === "function") {
    return globalThis.structuredClone(value);
  }
  return JSON.parse(JSON.stringify(value)) as T;
}

function cloneStructuredQueryFilter(
  filter: StructuredQueryFilter | undefined,
): StructuredQueryFilter | undefined {
  if (!filter) {
    return undefined;
  }
  if ("fieldFilter" in filter) {
    return {
      fieldFilter: {
        field: { fieldPath: filter.fieldFilter.field.fieldPath },
        op: filter.fieldFilter.op,
        value: cloneQueryValue(filter.fieldFilter.value),
      },
    };
  }
  return {
    compositeFilter: {
      op: "AND",
      filters: filter.compositeFilter.filters.map((entry) =>
        cloneStructuredQueryFilter(entry),
      ) as StructuredQueryFilter[],
    },
  };
}

export function cloneStructuredQueryShape(queryShape: StructuredQueryShape): StructuredQueryShape {
  return {
    endAt: queryShape.endAt
      ? {
          before: queryShape.endAt.before,
          values: queryShape.endAt.values.map((value) => cloneQueryValue(value)),
        }
      : undefined,
    from: queryShape.from.map((selector) => ({
      allDescendants: selector.allDescendants,
      collectionId: selector.collectionId,
    })),
    limit: queryShape.limit,
    orderBy: queryShape.orderBy?.map((order) => ({
      direction: order.direction,
      field: { fieldPath: order.field.fieldPath },
    })),
    startAt: queryShape.startAt
      ? {
          before: queryShape.startAt.before,
          values: queryShape.startAt.values.map((value) => cloneQueryValue(value)),
        }
      : undefined,
    where: cloneStructuredQueryFilter(queryShape.where),
  };
}

export function assertQueryFieldPath(
  fieldPath: string | DocumentIdFieldPath,
  context: string,
): string {
  if (isDocumentIdFieldPath(fieldPath)) {
    return "__name__";
  }
  const normalized = fieldPath.trim();
  if (normalized.length === 0) {
    throw new Error(`Firestore ${context} field paths must not be empty.`);
  }
  if (normalized.includes(".")) {
    throw new Error(
      `Firestore ${context} nested field paths are not supported yet.`,
    );
  }
  return normalized;
}

export function assertCursorValues(values: unknown[], name: string): readonly unknown[] {
  if (values.length === 0) {
    throw new Error(`Firestore ${name} requires at least one cursor value.`);
  }
  return values.map((value) => cloneQueryValue(value));
}

export function baseStructuredQuery<AppModelType = DocumentData>(
  source: QuerySource<AppModelType>,
): StructuredQueryShape {
  return {
    from: [
      isCollectionGroup(source)
        ? { allDescendants: true, collectionId: source.id }
        : { collectionId: source.id },
    ],
  };
}

function appendStructuredWhereFilter(
  existing: StructuredQueryFilter | undefined,
  nextFilter: StructuredQueryFilter,
): StructuredQueryFilter {
  if (!existing) {
    return nextFilter;
  }
  if ("compositeFilter" in existing && existing.compositeFilter.op === "AND") {
    return {
      compositeFilter: {
        op: "AND",
        filters: [...existing.compositeFilter.filters, nextFilter],
      },
    };
  }
  return {
    compositeFilter: {
      op: "AND",
      filters: [existing, nextFilter],
    },
  };
}

export function applyQueryConstraint(
  queryShape: StructuredQueryShape,
  constraint: QueryConstraint,
): StructuredQueryShape {
  if (constraint.type === "where") {
    const nextFilter: StructuredQueryFilter = {
      fieldFilter: {
        field: { fieldPath: constraint.fieldPath },
        op: WHERE_OPERATOR_MAP[constraint.op],
        value: cloneQueryValue(constraint.value),
      },
    };
    return {
      ...queryShape,
      where: appendStructuredWhereFilter(queryShape.where, nextFilter),
    };
  }

  if (constraint.type === "orderBy") {
    return {
      ...queryShape,
      orderBy: [
        ...(queryShape.orderBy ?? []),
        {
          direction: ORDER_DIRECTION_MAP[constraint.direction],
          field: { fieldPath: constraint.fieldPath },
        },
      ],
    };
  }

  if (constraint.type === "limit") {
    if (queryShape.limit !== undefined) {
      throw new Error("Firestore queries support at most one limit() constraint.");
    }
    return {
      ...queryShape,
      limit: constraint.count,
    };
  }

  if (constraint.kind === "startAt") {
    if (queryShape.startAt) {
      throw new Error(
        "Firestore queries support at most one startAt()/startAfter() constraint.",
      );
    }
    return {
      ...queryShape,
      startAt: {
        before: constraint.before,
        values: constraint.values,
      },
    };
  }

  if (queryShape.endAt) {
    throw new Error(
      "Firestore queries support at most one endAt()/endBefore() constraint.",
    );
  }
  return {
    ...queryShape,
    endAt: {
      before: constraint.before,
      values: constraint.values,
    },
  };
}

export function resolveAppAndDatabase(
  appOrDatabaseId?: FirebaseApp | string,
  databaseId?: string,
): { app: FirebaseApp; databaseId: string } {
  if (typeof appOrDatabaseId === "string") {
    return {
      app: getApp(),
      databaseId: normalizedDatabaseId(appOrDatabaseId),
    };
  }
  return {
    app: appOrDatabaseId ?? getApp(),
    databaseId: normalizedDatabaseId(databaseId),
  };
}

function requireProjectId(firestore: Firestore): string {
  const projectId = firestore.app.options.projectId?.trim();
  if (!projectId) {
    throw new Error(
      "Firestore operations require Firebase app options.projectId to be set.",
    );
  }
  return projectId;
}

export function databaseResourceName(firestore: Firestore): string {
  return `projects/${requireProjectId(firestore)}/databases/${firestore.databaseId}`;
}

function encodedPath(path: string): string {
  return path
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/");
}

export function queryRouteSuffix<AppModelType = DocumentData>(query: Query<AppModelType>): string {
  if (isCollectionReference(query.source) && query.source.parent) {
    return `/documents/${encodedPath(query.source.parent.path)}:runQuery`;
  }
  return "/documents:runQuery";
}

export function databaseBaseUrl(firestore: Firestore): string {
  const protocol = firestore.settings.ssl ? "https" : "http";
  return `${protocol}://${firestore.settings.host}/v1/${databaseResourceName(firestore)}`;
}

export function toFirestoreDocumentData<AppModelType = DocumentData>(
  converter: FirestoreDataConverter<AppModelType> | null,
  value: AppModelType,
  context: string,
): DocumentData {
  if (!converter) {
    return assertDocumentData(value, context);
  }
  return assertDocumentData(converter.toFirestore(value), context);
}

export function createAutoId(): string {
  const cryptoApi = globalThis.crypto;
  let id = "";
  if (cryptoApi && typeof cryptoApi.getRandomValues === "function") {
    const bytes = new Uint8Array(AUTO_ID_LENGTH);
    cryptoApi.getRandomValues(bytes);
    for (const byte of bytes) {
      id += AUTO_ID_ALPHABET[byte % AUTO_ID_ALPHABET.length];
    }
    return id;
  }
  for (let index = 0; index < AUTO_ID_LENGTH; index += 1) {
    id += AUTO_ID_ALPHABET[Math.floor(Math.random() * AUTO_ID_ALPHABET.length)];
  }
  return id;
}
