import {
  ARG_MARKER,
  HTTP_RESPONSE_MARKER,
  OPERATION_MARKER,
  REQUEST_MARKER,
  RESULT_MARKER,
} from "../constants.mjs";

function createArgsProxy(pathParts = []) {
  return new Proxy(
    {},
    {
      get(_target, property) {
        if (
          property === REQUEST_MARKER ||
          property === RESULT_MARKER ||
          property === OPERATION_MARKER ||
          property === HTTP_RESPONSE_MARKER
        ) {
          return undefined;
        }
        if (typeof property === "symbol") {
          return undefined;
        }
        const nextPath = [...pathParts, String(property)];
        const marker = Object.freeze({ [ARG_MARKER]: nextPath.join(".") });
        return new Proxy(marker, {
          get(innerTarget, innerProperty) {
            if (innerProperty === ARG_MARKER) {
              return innerTarget[ARG_MARKER];
            }
            if (
              innerProperty === REQUEST_MARKER ||
              innerProperty === RESULT_MARKER ||
              innerProperty === OPERATION_MARKER ||
              innerProperty === HTTP_RESPONSE_MARKER
            ) {
              return undefined;
            }
            if (typeof innerProperty === "symbol") {
              return undefined;
            }
            return createArgsProxy([...nextPath, String(innerProperty)]);
          },
        });
      },
    },
  );
}

export { createArgsProxy };
