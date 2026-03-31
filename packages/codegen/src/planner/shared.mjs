import { unsupportedError } from "../errors.mjs";

import { isArgMarker } from "./templates.mjs";

function normalizeTableName(value, filePath) {
  return normalizeString(value, filePath, "table name");
}

function normalizeString(value, filePath, label) {
  if (typeof value !== "string" || value.length === 0) {
    throw unsupportedError(filePath, `${label} must be a non-empty string literal`);
  }
  return value;
}

function normalizeRecord(value, filePath, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw unsupportedError(filePath, `${label} requires a plain object`);
  }
  return value;
}

function normalizeLimit(limit, filePath) {
  if (isArgMarker(limit)) {
    return limit;
  }
  if (!Number.isInteger(limit) || limit < 0) {
    throw unsupportedError(filePath, "take(limit) requires a non-negative integer");
  }
  return limit;
}

function normalizeOrderDirection(value, filePath) {
  if (value === "asc" || value === "desc") {
    return value;
  }
  throw unsupportedError(filePath, 'order(direction) requires "asc" or "desc"');
}

function normalizeDelayMs(value, filePath) {
  if (isArgMarker(value)) {
    return value;
  }
  if (!Number.isInteger(value) || value < 0) {
    throw unsupportedError(filePath, "runAfter(delayMs) requires a non-negative integer");
  }
  return value;
}

function normalizeTimestampMs(value, filePath) {
  if (isArgMarker(value)) {
    return value;
  }
  if (!Number.isInteger(value) || value < 0) {
    throw unsupportedError(filePath, "runAt(timestampMs) requires a non-negative integer");
  }
  return value;
}

function normalizeJobId(value, filePath) {
  if (isArgMarker(value)) {
    return value;
  }
  return normalizeString(value, filePath, "scheduled job id");
}

export {
  normalizeDelayMs,
  normalizeJobId,
  normalizeLimit,
  normalizeOrderDirection,
  normalizeRecord,
  normalizeString,
  normalizeTableName,
  normalizeTimestampMs,
};
