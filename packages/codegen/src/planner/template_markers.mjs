import {
  ARG_MARKER,
  HTTP_RESPONSE_MARKER,
  OPERATION_MARKER,
  QUERY_STATE_MARKER,
  REQUEST_MARKER,
  RESULT_MARKER,
} from "../constants.mjs";

function isArgMarker(value) {
  return (
    value &&
    typeof value === "object" &&
    Object.keys(value).length === 1 &&
    typeof value[ARG_MARKER] === "string"
  );
}

function isOperationMarker(value) {
  return (
    value &&
    typeof value === "object" &&
    Object.keys(value).length === 1 &&
    Number.isInteger(value[OPERATION_MARKER])
  );
}

function isRequestMarker(value) {
  return (
    value &&
    typeof value === "object" &&
    Object.keys(value).length === 1 &&
    value[REQUEST_MARKER] &&
    typeof value[REQUEST_MARKER] === "object" &&
    !Array.isArray(value[REQUEST_MARKER])
  );
}

function isResultMarker(value) {
  return (
    value &&
    typeof value === "object" &&
    Object.keys(value).length === 1 &&
    value[RESULT_MARKER] &&
    typeof value[RESULT_MARKER] === "object" &&
    !Array.isArray(value[RESULT_MARKER]) &&
    Number.isInteger(value[RESULT_MARKER].index)
  );
}

function isHttpResponseMarker(value) {
  return (
    value &&
    typeof value === "object" &&
    Object.keys(value).length === 1 &&
    value[HTTP_RESPONSE_MARKER] &&
    typeof value[HTTP_RESPONSE_MARKER] === "object" &&
    !Array.isArray(value[HTTP_RESPONSE_MARKER])
  );
}

function isQueryStateMarker(value) {
  return (
    value &&
    typeof value === "object" &&
    value[QUERY_STATE_MARKER] &&
    typeof value[QUERY_STATE_MARKER] === "object" &&
    !Array.isArray(value[QUERY_STATE_MARKER])
  );
}

export {
  isArgMarker,
  isHttpResponseMarker,
  isOperationMarker,
  isQueryStateMarker,
  isRequestMarker,
  isResultMarker,
};
