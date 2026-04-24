import ts from "typescript";

import { unsupportedError } from "./errors.mjs";

function evaluateCompileTimeExpressionSource(
  expressionText,
  compileBindings,
  filePath,
  label = "expression",
) {
  return evaluateExpression(
    parseCompileTimeExpressionSource(expressionText, filePath, label),
    createRootScope(compileBindings),
    filePath,
  );
}

function createInterpretedResolver(resolverNode, compileBindings, filePath) {
  const rootScope = createRootScope(compileBindings);
  return (...args) => invokeArrowFunction(resolverNode, args, rootScope, filePath);
}

function createRootScope(compileBindings) {
  const rootScope = new Scope(null);
  for (const [name, value] of Object.entries(compileBindings)) {
    rootScope.define(name, value);
  }
  return rootScope;
}

function parseCompileTimeExpressionSource(expressionText, filePath, label) {
  const sourceFile = ts.createSourceFile(
    `${filePath}.${label}.ts`,
    `const __neovexCompileTime = (${expressionText});`,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );

  if (sourceFile.parseDiagnostics.length > 0) {
    const diagnostic = sourceFile.parseDiagnostics[0];
    throw unsupportedError(
      filePath,
      `${label} parsing (${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")})`,
    );
  }

  const statement = sourceFile.statements[0];
  if (
    !statement
    || !ts.isVariableStatement(statement)
    || statement.declarationList.declarations.length !== 1
  ) {
    throw unsupportedError(filePath, `${label} parsing (unsupported expression wrapper)`);
  }

  const initializer = statement.declarationList.declarations[0].initializer;
  if (!initializer) {
    throw unsupportedError(filePath, `${label} parsing (missing expression initializer)`);
  }

  return initializer;
}

function invokeArrowFunction(node, args, parentScope, filePath) {
  const scope = new Scope(parentScope);
  bindParameters(node.parameters, args, scope, filePath);
  return evaluateFunctionBody(node.body, scope, filePath);
}

function evaluateFunctionBody(body, scope, filePath) {
  if (ts.isBlock(body)) {
    const result = evaluateStatement(body, scope, filePath);
    return result?.kind === "return" ? result.value : undefined;
  }
  return evaluateExpression(body, scope, filePath);
}

function evaluateStatement(statement, scope, filePath) {
  if (ts.isBlock(statement)) {
    const blockScope = new Scope(scope);
    for (const child of statement.statements) {
      const result = evaluateStatement(child, blockScope, filePath);
      if (result) {
        return result;
      }
    }
    return null;
  }

  if (ts.isReturnStatement(statement)) {
    return {
      kind: "return",
      value:
        statement.expression === undefined
          ? undefined
          : evaluateExpression(statement.expression, scope, filePath),
    };
  }

  if (ts.isExpressionStatement(statement)) {
    evaluateExpression(statement.expression, scope, filePath);
    return null;
  }

  if (ts.isVariableStatement(statement)) {
    for (const declaration of statement.declarationList.declarations) {
      if (declaration.initializer === undefined) {
        throw unsupportedError(filePath, "compile-time variables require initializers");
      }
      const value = evaluateExpression(declaration.initializer, scope, filePath);
      bindPattern(declaration.name, value, scope, filePath);
    }
    return null;
  }

  if (ts.isIfStatement(statement)) {
    const branch = isTruthy(evaluateExpression(statement.expression, scope, filePath))
      ? statement.thenStatement
      : statement.elseStatement;
    return branch ? evaluateStatement(branch, scope, filePath) : null;
  }

  throw unsupportedError(
    filePath,
    `unsupported compile-time resolver statement "${ts.SyntaxKind[statement.kind]}"`,
  );
}

function evaluateExpression(node, scope, filePath) {
  if (ts.isParenthesizedExpression(node)) {
    return evaluateExpression(node.expression, scope, filePath);
  }

  if (ts.isArrowFunction(node)) {
    return (...args) => invokeArrowFunction(node, args, scope, filePath);
  }

  if (
    ts.isAsExpression(node)
    || ts.isTypeAssertionExpression(node)
    || ts.isSatisfiesExpression(node)
    || ts.isNonNullExpression(node)
  ) {
    return evaluateExpression(node.expression, scope, filePath);
  }

  if (ts.isIdentifier(node)) {
    if (node.text === "undefined") {
      return undefined;
    }
    return scope.lookup(node.text);
  }

  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
    return node.text;
  }

  if (ts.isNumericLiteral(node)) {
    return Number(node.text.replaceAll("_", ""));
  }

  if (node.kind === ts.SyntaxKind.TrueKeyword) {
    return true;
  }

  if (node.kind === ts.SyntaxKind.FalseKeyword) {
    return false;
  }

  if (node.kind === ts.SyntaxKind.NullKeyword) {
    return null;
  }

  if (ts.isObjectLiteralExpression(node)) {
    return evaluateObjectLiteral(node, scope, filePath);
  }

  if (ts.isArrayLiteralExpression(node)) {
    return node.elements.map((element) => {
      if (ts.isSpreadElement(element)) {
        throw unsupportedError(filePath, "spread elements are not supported in compile-time arrays");
      }
      return evaluateExpression(element, scope, filePath);
    });
  }

  if (ts.isPropertyAccessExpression(node) || ts.isPropertyAccessChain(node)) {
    return evaluatePropertyAccess(node, scope, filePath);
  }

  if (ts.isElementAccessExpression(node) || ts.isElementAccessChain(node)) {
    return evaluateElementAccess(node, scope, filePath);
  }

  if (ts.isCallExpression(node) || ts.isCallChain(node)) {
    return evaluateCallExpression(node, scope, filePath);
  }

  if (ts.isNewExpression(node)) {
    return evaluateNewExpression(node, scope, filePath);
  }

  if (ts.isAwaitExpression(node)) {
    return evaluateExpression(node.expression, scope, filePath);
  }

  if (ts.isConditionalExpression(node)) {
    return isTruthy(evaluateExpression(node.condition, scope, filePath))
      ? evaluateExpression(node.whenTrue, scope, filePath)
      : evaluateExpression(node.whenFalse, scope, filePath);
  }

  if (ts.isBinaryExpression(node)) {
    return evaluateBinaryExpression(node, scope, filePath);
  }

  if (ts.isPrefixUnaryExpression(node)) {
    return evaluatePrefixUnaryExpression(node, scope, filePath);
  }

  if (ts.isTemplateExpression(node)) {
    let text = node.head.text;
    for (const span of node.templateSpans) {
      text += String(evaluateExpression(span.expression, scope, filePath));
      text += span.literal.text;
    }
    return text;
  }

  throw unsupportedError(
    filePath,
    `unsupported compile-time resolver expression "${ts.SyntaxKind[node.kind]}"`,
  );
}

function evaluateObjectLiteral(node, scope, filePath) {
  const objectValue = {};

  for (const property of node.properties) {
    if (ts.isPropertyAssignment(property)) {
      objectValue[propertyName(property.name, scope, filePath)] = evaluateExpression(
        property.initializer,
        scope,
        filePath,
      );
      continue;
    }

    if (ts.isShorthandPropertyAssignment(property)) {
      objectValue[property.name.text] = scope.lookup(property.name.text);
      continue;
    }

    throw unsupportedError(
      filePath,
      `unsupported compile-time object property "${ts.SyntaxKind[property.kind]}"`,
    );
  }

  return objectValue;
}

function evaluatePropertyAccess(node, scope, filePath) {
  const target = evaluateExpression(node.expression, scope, filePath);
  if (node.questionDotToken && target == null) {
    return undefined;
  }
  return target[node.name.text];
}

function evaluateElementAccess(node, scope, filePath) {
  const target = evaluateExpression(node.expression, scope, filePath);
  if (node.questionDotToken && target == null) {
    return undefined;
  }
  const key = evaluateExpression(node.argumentExpression, scope, filePath);
  return target[key];
}

function evaluateCallExpression(node, scope, filePath) {
  const callee = resolveCallee(node.expression, scope, filePath);
  if (callee.shortCircuit) {
    return undefined;
  }
  if (node.questionDotToken && callee.value == null) {
    return undefined;
  }
  if (typeof callee.value !== "function") {
    throw new TypeError(`${renderCallableExpression(node.expression)} is not a function`);
  }
  const args = node.arguments.map((argument) => evaluateExpression(argument, scope, filePath));
  return callee.value.apply(callee.thisArg, args);
}

function evaluateNewExpression(node, scope, filePath) {
  const callee = evaluateExpression(node.expression, scope, filePath);
  if (typeof callee !== "function") {
    throw new TypeError(`${renderCallableExpression(node.expression)} is not a constructor`);
  }
  const args = (node.arguments ?? []).map((argument) =>
    evaluateExpression(argument, scope, filePath));
  return new callee(...args);
}

function evaluateBinaryExpression(node, scope, filePath) {
  switch (node.operatorToken.kind) {
    case ts.SyntaxKind.AmpersandAmpersandToken: {
      const left = evaluateExpression(node.left, scope, filePath);
      return isTruthy(left) ? evaluateExpression(node.right, scope, filePath) : left;
    }
    case ts.SyntaxKind.BarBarToken: {
      const left = evaluateExpression(node.left, scope, filePath);
      return isTruthy(left) ? left : evaluateExpression(node.right, scope, filePath);
    }
    case ts.SyntaxKind.QuestionQuestionToken: {
      const left = evaluateExpression(node.left, scope, filePath);
      return left ?? evaluateExpression(node.right, scope, filePath);
    }
    default:
      return applyBinaryOperator(
        node.operatorToken.kind,
        evaluateExpression(node.left, scope, filePath),
        evaluateExpression(node.right, scope, filePath),
        filePath,
      );
  }
}

function applyBinaryOperator(operatorKind, left, right, filePath) {
  switch (operatorKind) {
    case ts.SyntaxKind.EqualsEqualsEqualsToken:
      return left === right;
    case ts.SyntaxKind.ExclamationEqualsEqualsToken:
      return left !== right;
    case ts.SyntaxKind.EqualsEqualsToken:
      return left == right;
    case ts.SyntaxKind.ExclamationEqualsToken:
      return left != right;
    case ts.SyntaxKind.PlusToken:
      return left + right;
    case ts.SyntaxKind.MinusToken:
      return left - right;
    case ts.SyntaxKind.AsteriskToken:
      return left * right;
    case ts.SyntaxKind.SlashToken:
      return left / right;
    case ts.SyntaxKind.PercentToken:
      return left % right;
    case ts.SyntaxKind.LessThanToken:
      return left < right;
    case ts.SyntaxKind.LessThanEqualsToken:
      return left <= right;
    case ts.SyntaxKind.GreaterThanToken:
      return left > right;
    case ts.SyntaxKind.GreaterThanEqualsToken:
      return left >= right;
    default:
      throw unsupportedError(
        filePath,
        `unsupported compile-time binary operator "${ts.SyntaxKind[operatorKind]}"`,
      );
  }
}

function evaluatePrefixUnaryExpression(node, scope, filePath) {
  const operand = evaluateExpression(node.operand, scope, filePath);
  switch (node.operator) {
    case ts.SyntaxKind.ExclamationToken:
      return !isTruthy(operand);
    case ts.SyntaxKind.PlusToken:
      return +operand;
    case ts.SyntaxKind.MinusToken:
      return -operand;
    case ts.SyntaxKind.TypeOfKeyword:
      return typeof operand;
    default:
      throw unsupportedError(
        filePath,
        `unsupported compile-time unary operator "${ts.SyntaxKind[node.operator]}"`,
      );
  }
}

function resolveCallee(expression, scope, filePath) {
  if (ts.isPropertyAccessExpression(expression) || ts.isPropertyAccessChain(expression)) {
    const target = evaluateExpression(expression.expression, scope, filePath);
    if (expression.questionDotToken && target == null) {
      return { shortCircuit: true, thisArg: undefined, value: undefined };
    }
    return {
      shortCircuit: false,
      thisArg: target,
      value: target[expression.name.text],
    };
  }

  if (ts.isElementAccessExpression(expression) || ts.isElementAccessChain(expression)) {
    const target = evaluateExpression(expression.expression, scope, filePath);
    if (expression.questionDotToken && target == null) {
      return { shortCircuit: true, thisArg: undefined, value: undefined };
    }
    const key = evaluateExpression(expression.argumentExpression, scope, filePath);
    return {
      shortCircuit: false,
      thisArg: target,
      value: target[key],
    };
  }

  return {
    shortCircuit: false,
    thisArg: undefined,
    value: evaluateExpression(expression, scope, filePath),
  };
}

function bindParameters(parameters, args, scope, filePath) {
  parameters.forEach((parameter, index) => {
    if (parameter.dotDotDotToken) {
      throw unsupportedError(filePath, "rest parameters are not supported in compile-time resolvers");
    }
    bindPattern(parameter.name, args[index], scope, filePath);
  });
}

function bindPattern(pattern, value, scope, filePath) {
  if (ts.isIdentifier(pattern)) {
    scope.define(pattern.text, value);
    return;
  }

  if (ts.isObjectBindingPattern(pattern)) {
    for (const element of pattern.elements) {
      if (element.dotDotDotToken) {
        throw unsupportedError(filePath, "rest bindings are not supported in compile-time resolvers");
      }
      const key = bindingElementName(element, filePath);
      bindPattern(element.name, value?.[key], scope, filePath);
    }
    return;
  }

  if (ts.isArrayBindingPattern(pattern)) {
    pattern.elements.forEach((element, index) => {
      if (ts.isOmittedExpression(element)) {
        return;
      }
      if (element.dotDotDotToken) {
        throw unsupportedError(filePath, "rest bindings are not supported in compile-time resolvers");
      }
      bindPattern(element.name, value?.[index], scope, filePath);
    });
    return;
  }

  throw unsupportedError(
    filePath,
    `unsupported compile-time binding pattern "${ts.SyntaxKind[pattern.kind]}"`,
  );
}

function bindingElementName(element, filePath) {
  if (element.propertyName === undefined) {
    if (!ts.isIdentifier(element.name)) {
      throw unsupportedError(filePath, "nested binding properties must use named keys");
    }
    return element.name.text;
  }
  return propertyName(element.propertyName, null, filePath);
}

function propertyName(name, scope, filePath) {
  if (ts.isIdentifier(name) || ts.isPrivateIdentifier(name)) {
    return name.text;
  }
  if (ts.isStringLiteral(name) || ts.isNoSubstitutionTemplateLiteral(name)) {
    return name.text;
  }
  if (ts.isNumericLiteral(name)) {
    return name.text;
  }
  if (ts.isComputedPropertyName(name) && scope !== null) {
    return String(evaluateExpression(name.expression, scope, filePath));
  }
  throw unsupportedError(filePath, "unsupported compile-time property name");
}

function isTruthy(value) {
  return !!value;
}

function renderCallableExpression(expression) {
  return expression.getText();
}

class Scope {
  constructor(parent) {
    this.parent = parent;
    this.bindings = new Map();
  }

  define(name, value) {
    this.bindings.set(name, value);
  }

  lookup(name) {
    if (this.bindings.has(name)) {
      return this.bindings.get(name);
    }
    if (this.parent) {
      return this.parent.lookup(name);
    }
    throw new ReferenceError(`${name} is not defined`);
  }
}

export { createInterpretedResolver, evaluateCompileTimeExpressionSource };
