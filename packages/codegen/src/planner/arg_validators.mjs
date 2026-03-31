import { ARG_MARKER } from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

import { isArgMarker } from "./templates.mjs";

function normalizeDocumentReference(value, argsSchema, filePath, label) {
  if (!isArgMarker(value)) {
    throw unsupportedError(
      filePath,
      `${label} requires an id argument declared with v.id("table") in 4B`,
    );
  }

  const validator = findArgValidator(argsSchema, value[ARG_MARKER], filePath, label);
  const resolved = unwrapOptionalValidator(validator);
  if (resolved.kind !== "id" || typeof resolved.tableName !== "string") {
    throw unsupportedError(
      filePath,
      `${label} requires an id argument declared with v.id("table") in 4B`,
    );
  }

  return {
    table: resolved.tableName,
    id: value,
  };
}

function findArgValidator(argsSchema, argPath, filePath, label) {
  const parts = argPath.split(".");
  let current = argsSchema;
  for (const part of parts) {
    if (!current || typeof current !== "object" || Array.isArray(current)) {
      throw unsupportedError(
        filePath,
        `${label} could not resolve validator metadata for "${argPath}"`,
      );
    }

    const next = current[part];
    if (next === undefined) {
      throw unsupportedError(
        filePath,
        `${label} could not resolve validator metadata for "${argPath}"`,
      );
    }

    current = unwrapOptionalValidator(next);
    if (current.kind === "object") {
      current = current.fields;
    }
  }

  if (!current || typeof current !== "object" || Array.isArray(current)) {
    throw unsupportedError(
      filePath,
      `${label} could not resolve validator metadata for "${argPath}"`,
    );
  }

  return current;
}

function unwrapOptionalValidator(validator) {
  let current = validator;
  while (current?.kind === "optional") {
    current = current.inner;
  }
  return current;
}

export { normalizeDocumentReference };
