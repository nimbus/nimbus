import type { DocumentData } from "../firestore";

type FieldValueKind =
  | "delete"
  | "serverTimestamp"
  | "increment"
  | "arrayUnion"
  | "arrayRemove";

const FIELD_VALUE_FACTORY = Symbol("neovex.firebase.FieldValue");

export class FieldValue {
  readonly _kind: FieldValueKind;
  readonly _operand: unknown;

  constructor(
    kind: FieldValueKind,
    operand: unknown,
    factory: symbol = Symbol("invalid"),
  ) {
    if (factory !== FIELD_VALUE_FACTORY) {
      throw new Error("FieldValue cannot be constructed directly.");
    }
    this._kind = kind;
    this._operand = operand;
    Object.freeze(this);
  }
}

export function deleteField(): FieldValue {
  return new FieldValue("delete", null, FIELD_VALUE_FACTORY);
}

export function serverTimestamp(): FieldValue {
  return new FieldValue("serverTimestamp", null, FIELD_VALUE_FACTORY);
}

export function increment(operand: number): FieldValue {
  if (typeof operand !== "number") {
    throw new Error("increment() requires a numeric operand.");
  }
  return new FieldValue("increment", operand, FIELD_VALUE_FACTORY);
}

export function arrayUnion(...elements: unknown[]): FieldValue {
  return new FieldValue("arrayUnion", elements, FIELD_VALUE_FACTORY);
}

export function arrayRemove(...elements: unknown[]): FieldValue {
  return new FieldValue("arrayRemove", elements, FIELD_VALUE_FACTORY);
}

export function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

export function assertDocumentData(value: unknown, context: string): DocumentData {
  if (!isPlainObject(value)) {
    throw new Error(`${context} must be a plain object.`);
  }
  return value;
}

export function isFieldValue(value: unknown): value is FieldValue {
  return value instanceof FieldValue;
}

export function splitFieldPath(fieldPath: string): string[] {
  if (fieldPath.trim().length === 0) {
    throw new Error("Firestore field paths must not be empty.");
  }
  const segments = fieldPath.split(".");
  if (segments.some((segment) => segment.length === 0)) {
    throw new Error(`Firestore field path "${fieldPath}" contains an empty segment.`);
  }
  return segments;
}

export function readValueAtFieldPath(
  source: Record<string, unknown>,
  segments: readonly string[],
): unknown {
  let cursor: unknown = source;
  for (const segment of segments) {
    if (!isPlainObject(cursor) || !(segment in cursor)) {
      return undefined;
    }
    cursor = cursor[segment];
  }
  return cursor;
}

export function setValueAtFieldPath(
  target: DocumentData,
  segments: readonly string[],
  value: unknown,
): void {
  let cursor: DocumentData = target;
  for (const segment of segments.slice(0, -1)) {
    const existing = cursor[segment];
    if (isPlainObject(existing)) {
      cursor = existing;
      continue;
    }
    const next: DocumentData = {};
    cursor[segment] = next;
    cursor = next;
  }
  cursor[segments.at(-1) ?? ""] = value;
}

export function hasFieldValueSentinel(value: unknown): boolean {
  if (isFieldValue(value)) {
    return true;
  }
  if (Array.isArray(value)) {
    return value.some((entry) => hasFieldValueSentinel(entry));
  }
  if (isPlainObject(value)) {
    return Object.values(value).some((entry) => hasFieldValueSentinel(entry));
  }
  return false;
}

function encodeInteger(value: number): string {
  if (!Number.isSafeInteger(value)) {
    throw new Error(
      `Firestore integer values must be safe JavaScript integers, received ${value}.`,
    );
  }
  return value.toString();
}

function encodeDouble(value: number): number | string {
  if (Number.isNaN(value)) {
    return "NaN";
  }
  if (value === Number.POSITIVE_INFINITY) {
    return "Infinity";
  }
  if (value === Number.NEGATIVE_INFINITY) {
    return "-Infinity";
  }
  if (Object.is(value, -0)) {
    return "-0";
  }
  return value;
}

export function encodeFirestoreValue(value: unknown): Record<string, unknown> {
  if (isFieldValue(value)) {
    throw new Error(
      "FieldValue sentinels must be used as direct document field values, not nested inside arrays or transform operands.",
    );
  }
  if (value === null) {
    return { nullValue: null };
  }
  if (typeof value === "boolean") {
    return { booleanValue: value };
  }
  if (typeof value === "number") {
    return Number.isInteger(value)
      ? { integerValue: encodeInteger(value) }
      : { doubleValue: encodeDouble(value) };
  }
  if (typeof value === "string") {
    return { stringValue: value };
  }
  if (Array.isArray(value)) {
    if (value.some(Array.isArray)) {
      throw new Error("Firestore arrays cannot directly contain arrays.");
    }
    return {
      arrayValue: {
        values: value.map((entry) => encodeFirestoreValue(entry)),
      },
    };
  }
  if (isPlainObject(value)) {
    return {
      mapValue: {
        fields: encodeDocumentFields(value),
      },
    };
  }

  throw new Error(
    `Unsupported Firestore value type: ${Object.prototype.toString.call(value)}.`,
  );
}

export function encodeDocumentFields(data: DocumentData): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(data).map(([key, value]) => [key, encodeFirestoreValue(value)]),
  );
}

function decodeFirestoreDouble(value: unknown): number {
  if (typeof value === "number") {
    return value;
  }
  if (value === "NaN") {
    return Number.NaN;
  }
  if (value === "Infinity") {
    return Number.POSITIVE_INFINITY;
  }
  if (value === "-Infinity") {
    return Number.NEGATIVE_INFINITY;
  }
  if (value === "-0") {
    return -0;
  }
  throw new Error(`Unsupported Firestore double value: ${String(value)}.`);
}

function decodeFirestoreValue(value: unknown): unknown {
  if (!isPlainObject(value) || Object.keys(value).length !== 1) {
    throw new Error("Firestore Value JSON must be an object with exactly one type field.");
  }
  const [kind, rawValue] = Object.entries(value)[0];
  switch (kind) {
    case "nullValue":
      return null;
    case "booleanValue":
      return Boolean(rawValue);
    case "integerValue":
      if (typeof rawValue !== "string") {
        throw new Error("Firestore integerValue must be a string.");
      }
      return Number.parseInt(rawValue, 10);
    case "doubleValue":
      return decodeFirestoreDouble(rawValue);
    case "timestampValue":
      return rawValue;
    case "stringValue":
      return rawValue;
    case "bytesValue":
      return rawValue;
    case "referenceValue":
      return rawValue;
    case "geoPointValue":
      return rawValue;
    case "arrayValue": {
      const values =
        isPlainObject(rawValue) && Array.isArray(rawValue.values) ? rawValue.values : [];
      return values.map((entry) => decodeFirestoreValue(entry));
    }
    case "mapValue": {
      const fields =
        isPlainObject(rawValue) && isPlainObject(rawValue.fields) ? rawValue.fields : {};
      return decodeDocumentFields(fields);
    }
    default:
      throw new Error(`Unsupported Firestore value type "${kind}".`);
  }
}

export function decodeDocumentFields(fields: Record<string, unknown>): DocumentData {
  return Object.fromEntries(
    Object.entries(fields).map(([key, value]) => [key, decodeFirestoreValue(value)]),
  );
}
