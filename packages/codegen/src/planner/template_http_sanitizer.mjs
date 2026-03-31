import {
  ARG_MARKER,
  HTTP_RESPONSE_MARKER,
  REQUEST_MARKER,
  RESULT_MARKER,
} from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

import {
  isArgMarker,
  isHttpResponseMarker,
  isOperationMarker,
  isRequestMarker,
  isResultMarker,
} from "./template_markers.mjs";

function sanitizeHttpActionPlan(resolved, operationLog, filePath) {
  if (!isHttpResponseMarker(resolved)) {
    throw unsupportedError(filePath, "httpAction handlers must return Response.json(...) or new Response(...)");
  }
  if (operationLog.length > 1) {
    throw unsupportedError(
      filePath,
      "httpAction handlers may compile at most one ctx.run*/ctx.scheduler operation in 4B",
    );
  }

  const response = sanitizeHttpResponseTemplate(resolved[HTTP_RESPONSE_MARKER], filePath);
  const plan = {
    type: "http_response",
    response,
  };
  if (operationLog.length === 1) {
    plan.operation = sanitizeHttpTemplateValue(operationLog[0], filePath);
  }
  return plan;
}

function sanitizeHttpResponseTemplate(template, filePath) {
  return {
    kind: template.kind,
    body: sanitizeHttpTemplateValue(template.body, filePath),
    status:
      template.status === undefined
        ? undefined
        : sanitizeHttpTemplateValue(template.status, filePath),
    headers:
      template.headers === undefined
        ? undefined
        : sanitizeHttpTemplateValue(template.headers, filePath),
  };
}

function sanitizeHttpTemplateValue(value, filePath) {
  if (value === null) {
    return null;
  }
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => sanitizeHttpTemplateValue(item, filePath));
  }
  if (typeof value === "object") {
    if (isArgMarker(value)) {
      return { $arg: value[ARG_MARKER] };
    }
    if (isRequestMarker(value)) {
      return { $request: value[REQUEST_MARKER] };
    }
    if (isResultMarker(value)) {
      return { $result: value[RESULT_MARKER] };
    }
    if (isOperationMarker(value)) {
      throw unsupportedError(
        filePath,
        "compiled operation results cannot be nested directly in httpAction responses",
      );
    }
    if (Object.getPrototypeOf(value) !== Object.prototype) {
      throw unsupportedError(filePath, "non-plain object in httpAction response");
    }

    const template = {};
    for (const [key, nested] of Object.entries(value)) {
      if (nested === undefined) {
        throw unsupportedError(filePath, `unsupported expression for "${key}"`);
      }
      template[key] = sanitizeHttpTemplateValue(nested, filePath);
    }
    return template;
  }

  throw unsupportedError(filePath, `unsupported value type "${typeof value}"`);
}

export { sanitizeHttpActionPlan };
