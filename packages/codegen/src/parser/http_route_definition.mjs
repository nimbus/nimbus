import { unsupportedError } from "../errors.mjs";
import {
  findTopLevelColon,
  parseStringLiteral,
  splitTopLevel,
  stripQuotes,
} from "../syntax.mjs";

function parseHttpRouteDefinition(definitionText, filePath) {
  if (!definitionText.startsWith("{") || !definitionText.endsWith("}")) {
    throw unsupportedError(filePath, "http route definition object");
  }

  const properties = {};
  for (const property of splitTopLevel(definitionText.slice(1, -1), ",", filePath)) {
    const separator = findTopLevelColon(property, filePath);
    const key = property.slice(0, separator).trim();
    const value = property.slice(separator + 1).trim();
    properties[stripQuotes(key)] = value;
  }

  const method = normalizeHttpMethod(properties.method, filePath);
  const path = properties.path !== undefined ? parseStringLiteral(properties.path, filePath) : undefined;
  const pathPrefix =
    properties.pathPrefix !== undefined
      ? parseStringLiteral(properties.pathPrefix, filePath)
      : undefined;

  if ((path === undefined && pathPrefix === undefined) || (path !== undefined && pathPrefix !== undefined)) {
    throw unsupportedError(filePath, "http routes must provide exactly one of path or pathPrefix");
  }
  if (path !== undefined && !path.startsWith("/")) {
    throw unsupportedError(filePath, "http route path must start with /");
  }
  if (pathPrefix !== undefined && !pathPrefix.startsWith("/")) {
    throw unsupportedError(filePath, "http route pathPrefix must start with /");
  }
  if (properties.handler === undefined) {
    throw unsupportedError(filePath, "http route must include a handler");
  }

  return {
    method,
    path,
    pathPrefix,
    handlerExpression: properties.handler,
  };
}

function normalizeHttpMethod(valueExpression, filePath) {
  const method = parseStringLiteral(valueExpression, filePath).toUpperCase();
  const supportedMethods = new Set([
    "GET",
    "POST",
    "PUT",
    "PATCH",
    "DELETE",
    "OPTIONS",
    "HEAD",
  ]);
  if (!supportedMethods.has(method)) {
    throw unsupportedError(filePath, `unsupported HTTP method "${method}"`);
  }
  return method;
}

export { parseHttpRouteDefinition };
