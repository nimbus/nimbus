import { convexValidators, sanitizeValidator } from "../schema.mjs";
import {
  findTopLevelColon,
  splitTopLevel,
  stripQuotes,
} from "../syntax.mjs";
import { unsupportedError } from "../errors.mjs";

function parseServerDefinition(definitionText, filePath, compileBindings = {}) {
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
    argsSchema: parseArgsSchema(properties.args, filePath, compileBindings),
    returnsSchema: parseReturnsSchema(properties.returns, filePath, compileBindings),
  };
}

function parseArgsSchema(argsExpression, filePath, compileBindings) {
  if (argsExpression === undefined) {
    return {};
  }

  const argsDefinition = evaluateValidatorExpression(
    argsExpression,
    filePath,
    compileBindings,
    "args",
  );

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

function parseReturnsSchema(returnsExpression, filePath, compileBindings) {
  if (returnsExpression === undefined) {
    return undefined;
  }

  return sanitizeValidator(
    evaluateValidatorExpression(returnsExpression, filePath, compileBindings, "returns"),
    filePath,
  );
}

function evaluateValidatorExpression(expression, filePath, compileBindings, label) {
  const bindings = {
    v: convexValidators,
    ...compileBindings,
  };
  const bindingNames = Object.keys(bindings);
  const bindingValues = bindingNames.map((name) => bindings[name]);

  try {
    return new Function(...bindingNames, `return (${expression});`)(...bindingValues);
  } catch (error) {
    throw unsupportedError(filePath, `${label} parsing (${error.message})`);
  }
}

export { parseServerDefinition };
