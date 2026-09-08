import {
  ARG_MARKER,
  HTTP_RESPONSE_MARKER,
  OPERATION_MARKER,
  REQUEST_MARKER,
  RESULT_MARKER,
} from "../constants.mjs";

// A handler argument has no value at compile time. The planner records the
// argument's path and lets it flow into a plan template; the moment the value
// would *decide* something (a branch, a default, a comparison, arithmetic) the
// handler can only run at runtime. This probe lets the interpreter tell an
// argument-derived value from a compile-time value without touching any
// string property, which the proxies below turn into further path markers.
const ARGS_DERIVED_PROBE = Symbol.for("nimbus.codegen.args-derived");

function isArgumentDerived(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    value[ARGS_DERIVED_PROBE] === true
  );
}

function createArgsProxy(pathParts = []) {
  return new Proxy(
    {},
    {
      get(_target, property) {
        if (property === ARGS_DERIVED_PROBE) {
          return true;
        }
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
            if (innerProperty === ARGS_DERIVED_PROBE) {
              return true;
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

export { createArgsProxy, isArgumentDerived };
