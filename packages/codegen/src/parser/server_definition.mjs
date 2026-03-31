import { convexValidators, sanitizeValidator } from "../schema.mjs";
import {
  findTopLevelColon,
  splitTopLevel,
  stripQuotes,
} from "../syntax.mjs";
import { unsupportedError } from "../errors.mjs";

function parseServerDefinition(definitionText, filePath) {
  if (!definitionText.startsWith("{") || !definitionText.endsWith("}")) {
    throw unsupportedError(filePath, "server function definition object");
  }

  const properties = {};
  for (const property of splitTopLevel(definitionText.slice(1, -1), ",", filePath)) {
    const separator = findTopLevelColon(property, filePath);
    const key = property.slice(0, separator).trim();
    const value = property.slice(separator + 1).trim();
    properties[stripQuotes(key)] = value;
  }

  const handlerExpression = properties.handler;
  if (handlerExpression === undefined) {
    throw unsupportedError(filePath, "missing handler property");
  }

  return {
    handlerExpression,
    argsSchema: parseArgsSchema(properties.args, filePath),
    returnsSchema: parseReturnsSchema(properties.returns, filePath),
  };
}

function parseArgsSchema(argsExpression, filePath) {
  if (argsExpression === undefined) {
    return {};
  }

  let argsDefinition;
  try {
    argsDefinition = new Function("v", `return (${argsExpression});`)(convexValidators);
  } catch (error) {
    throw unsupportedError(filePath, `args parsing (${error.message})`);
  }

  if (
    !argsDefinition ||
    typeof argsDefinition !== "object" ||
    Array.isArray(argsDefinition)
  ) {
    throw unsupportedError(filePath, "args must be an object literal");
  }

  const sanitized = {};
  for (const [fieldName, validator] of Object.entries(argsDefinition)) {
    sanitized[fieldName] = sanitizeValidator(validator, filePath);
  }
  return sanitized;
}

function parseReturnsSchema(returnsExpression, filePath) {
  if (returnsExpression === undefined) {
    return undefined;
  }

  let returnsDefinition;
  try {
    returnsDefinition = new Function("v", `return (${returnsExpression});`)(convexValidators);
  } catch (error) {
    throw unsupportedError(filePath, `returns parsing (${error.message})`);
  }

  return sanitizeValidator(returnsDefinition, filePath);
}

export { parseServerDefinition };
