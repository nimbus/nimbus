import {
  HTTP_RESPONSE_MARKER,
  REQUEST_MARKER,
} from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

import { isArgMarker, isRequestMarker, isResultMarker } from "./templates.mjs";
import { normalizeString } from "./shared.mjs";
import { createRequestMarker } from "./request_proxy.mjs";

function createHttpCompileBindings(filePath) {
  return {
    Response: createConvexResponseClass(filePath),
    URL: createConvexUrlClass(filePath),
  };
}

function createConvexResponseClass(filePath) {
  return class ConvexResponse {
    constructor(body = null, init = {}) {
      return createHttpResponseMarker("text", body, init, filePath);
    }

    static json(body, init = {}) {
      return createHttpResponseMarker("json", body, init, filePath);
    }
  };
}

function createConvexUrlClass(filePath) {
  return class ConvexUrl {
    constructor(input) {
      if (!isRequestMarker(input) || input[REQUEST_MARKER].source !== "url") {
        throw unsupportedError(filePath, "new URL(...) in httpAction requires request.url in 4B");
      }
    }

    get searchParams() {
      return {
        get(name) {
          return createRequestMarker({
            source: "query",
            name: normalizeString(name, filePath, "query parameter name"),
          });
        },
      };
    }

    get pathname() {
      return createRequestMarker({ source: "pathname" });
    }
  };
}

function createHttpResponseMarker(kind, body, init, filePath) {
  return Object.freeze({
    [HTTP_RESPONSE_MARKER]: {
      kind,
      body,
      status: normalizeResponseStatus(init?.status, filePath),
      headers: normalizeResponseHeaders(init?.headers, filePath),
    },
  });
}

function normalizeResponseStatus(value, filePath) {
  if (value === undefined) {
    return undefined;
  }
  if (isArgMarker(value) || isRequestMarker(value) || isResultMarker(value)) {
    return value;
  }
  if (!Number.isInteger(value) || value < 100 || value > 599) {
    throw unsupportedError(filePath, "Response status must be an integer HTTP status code");
  }
  return value;
}

function normalizeResponseHeaders(value, filePath) {
  if (value === undefined) {
    return undefined;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw unsupportedError(filePath, "Response headers must be a plain object in 4B");
  }
  return value;
}

export { createHttpCompileBindings };
