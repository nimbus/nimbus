import type {
  DocumentData,
  DocumentReference,
  SetOptions,
} from "../firestore";
import {
  type FieldValue,
  encodeDocumentFields,
  encodeFirestoreValue,
  hasFieldValueSentinel,
  isFieldValue,
  isPlainObject,
  readValueAtFieldPath,
  setValueAtFieldPath,
  splitFieldPath,
} from "./document-data";

export interface FirestoreWriteDependencies {
  documentResourceName<AppModelType = DocumentData>(
    reference: DocumentReference<AppModelType>,
  ): string;
}

interface ExtractedWriteData {
  readonly documentData: DocumentData;
  readonly deleteFieldPaths: readonly string[];
  readonly regularFieldPaths: readonly string[];
  readonly transformWrites: readonly Record<string, unknown>[];
}

interface MutableExtractedWriteData {
  documentData: DocumentData;
  deleteFieldPaths: string[];
  regularFieldPaths: string[];
  transformWrites: Record<string, unknown>[];
}

function createWriteExtractionState(): MutableExtractedWriteData {
  return {
    documentData: {},
    deleteFieldPaths: [],
    regularFieldPaths: [],
    transformWrites: [],
  };
}

function finalizeWriteExtraction(state: MutableExtractedWriteData): ExtractedWriteData {
  const deleteFieldPaths = Array.from(new Set(state.deleteFieldPaths)).sort();
  const regularFieldPaths = Array.from(new Set(state.regularFieldPaths)).sort();
  const transformWrites = Array.from(
    new Map(
      state.transformWrites.map((transformWrite) => [
        String(transformWrite.fieldPath),
        transformWrite,
      ]),
    ).values(),
  ).sort((left, right) =>
    String(left.fieldPath).localeCompare(String(right.fieldPath)),
  );

  return {
    documentData: state.documentData,
    deleteFieldPaths,
    regularFieldPaths,
    transformWrites,
  };
}

function encodeFieldTransform(fieldPath: string, value: FieldValue): Record<string, unknown> {
  switch (value._kind) {
    case "serverTimestamp":
      return {
        fieldPath,
        setToServerValue: "REQUEST_TIME",
      };
    case "increment":
      return {
        fieldPath,
        increment: encodeFirestoreValue(value._operand),
      };
    case "arrayUnion":
      return {
        appendMissingElements: {
          values: (value._operand as readonly unknown[]).map((entry) =>
            encodeFirestoreValue(entry),
          ),
        },
        fieldPath,
      };
    case "arrayRemove":
      return {
        fieldPath,
        removeAllFromArray: {
          values: (value._operand as readonly unknown[]).map((entry) =>
            encodeFirestoreValue(entry),
          ),
        },
      };
    case "delete":
      return {
        fieldPath,
      };
  }
}

function extractWriteValue(
  fieldPath: string,
  pathSegments: readonly string[],
  value: unknown,
  state: MutableExtractedWriteData,
): void {
  if (isFieldValue(value)) {
    if (value._kind === "delete") {
      state.deleteFieldPaths.push(fieldPath);
      return;
    }
    state.transformWrites.push(encodeFieldTransform(fieldPath, value));
    return;
  }

  if (isPlainObject(value)) {
    const entries = Object.entries(value);
    if (entries.length === 0) {
      setValueAtFieldPath(state.documentData, pathSegments, {});
      state.regularFieldPaths.push(fieldPath);
      return;
    }
    for (const [key, child] of entries) {
      const childSegments = [...pathSegments, key];
      const childFieldPath = fieldPath === "" ? key : `${fieldPath}.${key}`;
      extractWriteValue(childFieldPath, childSegments, child, state);
    }
    return;
  }

  setValueAtFieldPath(state.documentData, pathSegments, value);
  state.regularFieldPaths.push(fieldPath);
}

function extractSetData(source: DocumentData): ExtractedWriteData {
  const state = createWriteExtractionState();
  for (const [key, value] of Object.entries(source)) {
    extractWriteValue(key, [key], value, state);
  }
  return finalizeWriteExtraction(state);
}

function extractUpdateData(source: DocumentData): ExtractedWriteData {
  const state = createWriteExtractionState();
  for (const [key, value] of Object.entries(source)) {
    if (key.includes(".")) {
      const segments = splitFieldPath(key);
      extractWriteValue(segments.join("."), segments, value, state);
      continue;
    }
    extractWriteValue(key, [key], value, state);
  }
  return finalizeWriteExtraction(state);
}

function assertNoOverlappingFieldPaths(
  fieldPaths: readonly string[],
  context: string,
): void {
  const normalized = Array.from(new Set(fieldPaths)).sort();
  for (let index = 0; index < normalized.length; index += 1) {
    const current = normalized[index];
    for (let nextIndex = index + 1; nextIndex < normalized.length; nextIndex += 1) {
      const next = normalized[nextIndex];
      if (next.startsWith(`${current}.`)) {
        throw new Error(
          `${context} cannot address both "${current}" and "${next}" in the same write.`,
        );
      }
    }
  }
}

function assertNoConflictingWriteTargets(
  regularFieldPaths: readonly string[],
  deleteFieldPaths: readonly string[],
  transformWrites: readonly Record<string, unknown>[],
  context: string,
): void {
  const seen = new Map<string, string>();
  const claim = (fieldPath: string, kind: string) => {
    const existing = seen.get(fieldPath);
    if (existing && existing !== kind) {
      throw new Error(
        `${context} cannot apply both ${existing} and ${kind} to "${fieldPath}" in the same write.`,
      );
    }
    seen.set(fieldPath, kind);
  };

  for (const fieldPath of regularFieldPaths) {
    claim(fieldPath, "a regular value");
  }
  for (const fieldPath of deleteFieldPaths) {
    claim(fieldPath, "deleteField()");
  }
  for (const transformWrite of transformWrites) {
    claim(String(transformWrite.fieldPath), "a transform");
  }
}

function filterMergeFieldData(
  source: DocumentData,
  extracted: ExtractedWriteData,
  requestedFieldPaths: readonly string[],
): ExtractedWriteData {
  const requested = Array.from(
    new Set(requestedFieldPaths.map((fieldPath) => splitFieldPath(fieldPath).join("."))),
  ).sort();
  assertNoOverlappingFieldPaths(requested, "setDoc mergeFields");

  const deleteFieldPathSet = new Set(extracted.deleteFieldPaths);
  const transformWriteMap = new Map(
    extracted.transformWrites.map((transformWrite) => [
      String(transformWrite.fieldPath),
      transformWrite,
    ]),
  );

  const maskedDocumentData: DocumentData = {};
  const regularFieldPaths: string[] = [];
  const deleteFieldPaths: string[] = [];
  const transformWrites: Record<string, unknown>[] = [];

  for (const fieldPath of requested) {
    if (deleteFieldPathSet.has(fieldPath)) {
      deleteFieldPaths.push(fieldPath);
      continue;
    }
    const transformWrite = transformWriteMap.get(fieldPath);
    if (transformWrite) {
      transformWrites.push(transformWrite);
      continue;
    }

    const segments = splitFieldPath(fieldPath);
    const value = readValueAtFieldPath(source, segments);
    if (value === undefined) {
      throw new Error(
        `setDoc mergeFields path "${fieldPath}" was not present in the provided data.`,
      );
    }
    if (hasFieldValueSentinel(value)) {
      throw new Error(
        `setDoc mergeFields path "${fieldPath}" cannot target a subtree containing FieldValue sentinels; specify the exact leaf field paths instead.`,
      );
    }
    setValueAtFieldPath(maskedDocumentData, segments, value);
    regularFieldPaths.push(fieldPath);
  }

  return {
    documentData: maskedDocumentData,
    deleteFieldPaths,
    regularFieldPaths,
    transformWrites,
  };
}

function normalizeSetOptions(options?: SetOptions): SetOptions | undefined {
  if (!options) {
    return undefined;
  }
  if (options.merge && options.mergeFields !== undefined) {
    throw new Error("setDoc options must not combine merge and mergeFields.");
  }
  return options;
}

export function buildSetWrite<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  data: DocumentData,
  options: SetOptions | undefined,
  dependencies: FirestoreWriteDependencies,
): Record<string, unknown> {
  const normalizedOptions = normalizeSetOptions(options);
  const extracted = extractSetData(data);
  assertNoConflictingWriteTargets(
    extracted.regularFieldPaths,
    extracted.deleteFieldPaths,
    extracted.transformWrites,
    "setDoc",
  );
  assertNoOverlappingFieldPaths(
    [
      ...extracted.regularFieldPaths,
      ...extracted.deleteFieldPaths,
      ...extracted.transformWrites.map((transformWrite) => String(transformWrite.fieldPath)),
    ],
    "setDoc",
  );
  if (
    extracted.deleteFieldPaths.length > 0 &&
    !normalizedOptions?.merge &&
    normalizedOptions?.mergeFields === undefined
  ) {
    throw new Error(
      "deleteField() can only be used with updateDoc() or setDoc() when merge or mergeFields is enabled.",
    );
  }

  const write: Record<string, unknown> = {
    update: {
      name: dependencies.documentResourceName(reference),
      fields: encodeDocumentFields(extracted.documentData),
    },
  };

  if (normalizedOptions?.merge) {
    write.updateMask = {
      fieldPaths: [...extracted.regularFieldPaths, ...extracted.deleteFieldPaths],
    };
  } else if (normalizedOptions?.mergeFields) {
    const filtered = filterMergeFieldData(data, extracted, normalizedOptions.mergeFields);
    write.update = {
      name: dependencies.documentResourceName(reference),
      fields: encodeDocumentFields(filtered.documentData),
    };
    write.updateMask = {
      fieldPaths: [...filtered.regularFieldPaths, ...filtered.deleteFieldPaths],
    };
    if (filtered.transformWrites.length > 0) {
      write.updateTransforms = filtered.transformWrites;
    }
    return write;
  }

  if (
    normalizedOptions?.merge &&
    extracted.regularFieldPaths.length === 0 &&
    extracted.deleteFieldPaths.length === 0
  ) {
    write.updateMask = { fieldPaths: [] };
  }

  if (extracted.transformWrites.length > 0) {
    write.updateTransforms = extracted.transformWrites;
  }

  return write;
}

export function buildUpdateWrite<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  data: DocumentData,
  dependencies: FirestoreWriteDependencies,
): Record<string, unknown> {
  const extracted = extractUpdateData(data);
  assertNoConflictingWriteTargets(
    extracted.regularFieldPaths,
    extracted.deleteFieldPaths,
    extracted.transformWrites,
    "updateDoc",
  );
  const fieldPaths = Array.from(
    new Set([...extracted.regularFieldPaths, ...extracted.deleteFieldPaths]),
  ).sort();
  assertNoOverlappingFieldPaths(
    [
      ...fieldPaths,
      ...extracted.transformWrites.map((transformWrite) => String(transformWrite.fieldPath)),
    ],
    "updateDoc",
  );
  if (fieldPaths.length === 0 && extracted.transformWrites.length === 0) {
    throw new Error("updateDoc requires at least one field.");
  }
  return {
    update: {
      name: dependencies.documentResourceName(reference),
      fields: encodeDocumentFields(extracted.documentData),
    },
    updateMask: {
      fieldPaths,
    },
    ...(extracted.transformWrites.length > 0
      ? { updateTransforms: extracted.transformWrites }
      : {}),
    currentDocument: {
      exists: true,
    },
  };
}

export function buildDeleteWrite<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  dependencies: FirestoreWriteDependencies,
): Record<string, unknown> {
  return {
    delete: dependencies.documentResourceName(reference),
  };
}

export function buildCreateWrite<AppModelType = DocumentData>(
  reference: DocumentReference<AppModelType>,
  data: DocumentData,
  dependencies: FirestoreWriteDependencies,
): Record<string, unknown> {
  const extracted = extractSetData(data);
  assertNoConflictingWriteTargets(
    extracted.regularFieldPaths,
    extracted.deleteFieldPaths,
    extracted.transformWrites,
    "addDoc",
  );
  assertNoOverlappingFieldPaths(
    [
      ...extracted.regularFieldPaths,
      ...extracted.deleteFieldPaths,
      ...extracted.transformWrites.map((transformWrite) => String(transformWrite.fieldPath)),
    ],
    "addDoc",
  );
  if (extracted.deleteFieldPaths.length > 0) {
    throw new Error(
      "deleteField() can only be used with updateDoc() or setDoc() when merge or mergeFields is enabled.",
    );
  }
  return {
    update: {
      name: dependencies.documentResourceName(reference),
      fields: encodeDocumentFields(extracted.documentData),
    },
    ...(extracted.transformWrites.length > 0
      ? { updateTransforms: extracted.transformWrites }
      : {}),
    currentDocument: {
      exists: false,
    },
  };
}
