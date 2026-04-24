import {
  ARG_MARKER,
  HTTP_RESPONSE_MARKER,
  OPERATION_MARKER,
  REQUEST_MARKER,
  RESULT_MARKER,
} from "../constants.mjs";

import { normalizeString } from "./shared.mjs";

function createRequestProxy(filePath) {
  return {
    method: createRequestMarker({ source: "method" }),
    url: createRequestMarker({ source: "url" }),
    headers: {
      get(name) {
        return createRequestMarker({
          source: "header",
          name: normalizeString(name, filePath, "request header name"),
        });
      },
    },
    json() {
      return createRequestJsonProxy();
    },
    text() {
      return createRequestMarker({ source: "text" });
    },
  };
}

function createRequestJsonProxy(pathParts = []) {
  return createRequestMarker({
    source: "json",
    path: pathParts.join("."),
  });
}

function createRequestMarker(descriptor) {
  const marker = Object.freeze({ [REQUEST_MARKER]: descriptor });
  return new Proxy(marker, {
    get(target, property) {
      if (property === REQUEST_MARKER) {
        return target[REQUEST_MARKER];
      }
      if (
        property === ARG_MARKER ||
        property === RESULT_MARKER ||
        property === OPERATION_MARKER ||
        property === HTTP_RESPONSE_MARKER
      ) {
        return undefined;
      }
      if (property === "then" || typeof property === "symbol") {
        return undefined;
      }
      if (target[REQUEST_MARKER].source === "json") {
        return createRequestMarker({
          source: "json",
          path: [
            target[REQUEST_MARKER].path,
            String(property),
          ]
            .filter(Boolean)
            .join("."),
        });
      }
      return undefined;
    },
  });
}

export { createRequestMarker, createRequestProxy };
