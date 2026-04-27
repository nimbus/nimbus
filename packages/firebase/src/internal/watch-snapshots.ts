import type {
  DocumentData,
  DocumentReference,
  DocumentSnapshot,
  Query,
  QuerySnapshot,
} from "../firestore";

export interface FirestoreWatchSnapshotDependencies {
  buildDocumentSnapshot<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
    documentData: DocumentData | undefined,
  ): DocumentSnapshot<AppModelType>;
  buildQuerySnapshot<AppModelType = DocumentData>(
    query: Query<AppModelType>,
    documents: readonly { name: string; documentData: DocumentData }[],
  ): QuerySnapshot<AppModelType>;
  deepEqualValue(left: unknown, right: unknown): boolean;
  isPlainObject(value: unknown): value is Record<string, unknown>;
  readValueAtFieldPath(
    source: Record<string, unknown>,
    segments: readonly string[],
  ): unknown;
  splitFieldPath(fieldPath: string): string[];
}

export function buildWatchDocumentSnapshot<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  documentData: DocumentData | undefined,
  dependencies: FirestoreWatchSnapshotDependencies,
): DocumentSnapshot<AppModelType> {
  return dependencies.buildDocumentSnapshot(reference, documentData);
}

export function buildWatchQuerySnapshot<AppModelType = DocumentData>(
  query: Query<AppModelType>,
  documents: Map<string, DocumentData>,
  dependencies: FirestoreWatchSnapshotDependencies,
): QuerySnapshot<AppModelType> {
  const orderedDocuments = Array.from(documents.entries())
    .sort(([leftName, leftData], [rightName, rightData]) =>
      compareWatchedQueryDocuments(
        query,
        leftName,
        leftData,
        rightName,
        rightData,
        dependencies,
      ),
    )
    .map(([name, documentData]) => ({ name, documentData }));
  return dependencies.buildQuerySnapshot(query, orderedDocuments);
}

function compareWatchedQueryDocuments<AppModelType = DocumentData>(
  query: Query<AppModelType>,
  leftName: string,
  leftData: DocumentData,
  rightName: string,
  rightData: DocumentData,
  dependencies: FirestoreWatchSnapshotDependencies,
): number {
  for (const order of query.structuredQuery.orderBy ?? []) {
    const leftValue =
      order.field.fieldPath === "__name__"
        ? leftName
        : dependencies.readValueAtFieldPath(
            leftData,
            dependencies.splitFieldPath(order.field.fieldPath),
          );
    const rightValue =
      order.field.fieldPath === "__name__"
        ? rightName
        : dependencies.readValueAtFieldPath(
            rightData,
            dependencies.splitFieldPath(order.field.fieldPath),
          );
    const comparison = compareFirestoreOrderedValues(leftValue, rightValue, dependencies);
    if (comparison !== 0) {
      return order.direction === "DESCENDING" ? -comparison : comparison;
    }
  }
  return leftName.localeCompare(rightName);
}

function compareFirestoreOrderedValues(
  left: unknown,
  right: unknown,
  dependencies: FirestoreWatchSnapshotDependencies,
): number {
  if (dependencies.deepEqualValue(left, right)) {
    return 0;
  }
  const leftRank = firestoreValueSortRank(left, dependencies);
  const rightRank = firestoreValueSortRank(right, dependencies);
  if (leftRank !== rightRank) {
    return leftRank - rightRank;
  }
  if (typeof left === "number" && typeof right === "number") {
    return left < right ? -1 : 1;
  }
  if (typeof left === "string" && typeof right === "string") {
    return left.localeCompare(right);
  }
  if (typeof left === "boolean" && typeof right === "boolean") {
    return Number(left) - Number(right);
  }
  return JSON.stringify(left).localeCompare(JSON.stringify(right));
}

function firestoreValueSortRank(
  value: unknown,
  dependencies: FirestoreWatchSnapshotDependencies,
): number {
  if (value === undefined) {
    return 0;
  }
  if (value === null) {
    return 1;
  }
  if (typeof value === "boolean") {
    return 2;
  }
  if (typeof value === "number") {
    return 3;
  }
  if (typeof value === "string") {
    return 4;
  }
  if (Array.isArray(value)) {
    return 5;
  }
  if (dependencies.isPlainObject(value)) {
    return 6;
  }
  return 7;
}
