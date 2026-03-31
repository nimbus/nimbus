import {
  ARG_MARKER,
  HTTP_RESPONSE_MARKER,
  OPERATION_MARKER,
  REQUEST_MARKER,
  RESULT_MARKER,
} from "../constants.mjs";

function recordOperation(operationLog, operation, returnResultMarker = false) {
  const index = operationLog.push(operation) - 1;
  return returnResultMarker
    ? createResultProxy(index)
    : Object.freeze({ [OPERATION_MARKER]: index });
}

function createResultProxy(index, pathParts = []) {
  const marker = Object.freeze({
    [RESULT_MARKER]: {
      index,
      path: pathParts.join("."),
    },
  });
  return new Proxy(marker, {
    get(target, property) {
      if (property === RESULT_MARKER) {
        return target[RESULT_MARKER];
      }
      if (
        property === ARG_MARKER ||
        property === REQUEST_MARKER ||
        property === OPERATION_MARKER ||
        property === HTTP_RESPONSE_MARKER
      ) {
        return undefined;
      }
      if (property === "then" || typeof property === "symbol") {
        return undefined;
      }
      return createResultProxy(index, [...pathParts, String(property)]);
    },
  });
}

export { recordOperation };
