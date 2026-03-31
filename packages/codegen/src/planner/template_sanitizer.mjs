import { ARG_MARKER } from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

import { isArgMarker, isOperationMarker } from "./template_markers.mjs";

function sanitizeTemplate(value, filePath) {
  if (value === null) {
    return null;
  }
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => sanitizeTemplate(item, filePath));
  }
  if (typeof value === "object") {
    if (isArgMarker(value)) {
      return { $arg: value[ARG_MARKER] };
    }
    if (isOperationMarker(value)) {
      throw unsupportedError(
        filePath,
        "compiled operation results cannot be nested inside other returned values in 4B",
      );
    }
    if (Object.getPrototypeOf(value) !== Object.prototype) {
      throw unsupportedError(filePath, "non-plain object in resolver result");
    }

    const template = {};
    for (const [key, nested] of Object.entries(value)) {
      if (nested === undefined) {
        throw unsupportedError(filePath, `unsupported expression for "${key}"`);
      }
      template[key] = sanitizeTemplate(nested, filePath);
    }
    return template;
  }

  throw unsupportedError(filePath, `unsupported value type "${typeof value}"`);
}

export { sanitizeTemplate };
