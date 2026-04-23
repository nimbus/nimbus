import ts from "typescript";

function extractExportedConstAssignments(source, filePath) {
  const sourceFile = ts.createSourceFile(
    filePath,
    source,
    ts.ScriptTarget.Latest,
    true,
    scriptKindForPath(filePath),
  );
  const assignments = [];

  for (const statement of sourceFile.statements) {
    if (!isExported(statement) || !ts.isVariableStatement(statement)) {
      continue;
    }
    for (const declaration of statement.declarationList.declarations) {
      if (!ts.isIdentifier(declaration.name)) {
        assignments.push({
          exportName: null,
          helperName: null,
          callExpression: null,
        });
        continue;
      }
      const initializer = declaration.initializer;
      const helperName =
        initializer &&
        ts.isCallExpression(initializer) &&
        ts.isIdentifier(initializer.expression)
          ? initializer.expression.text
          : null;
      assignments.push({
        exportName: declaration.name.text,
        helperName,
        callExpression:
          initializer && ts.isCallExpression(initializer)
            ? initializer.getText(sourceFile)
            : null,
      });
    }
  }

  return assignments;
}

function hasUnsupportedExportShape(source, filePath) {
  const sourceFile = ts.createSourceFile(
    filePath,
    source,
    ts.ScriptTarget.Latest,
    true,
    scriptKindForPath(filePath),
  );

  return sourceFile.statements.some((statement) => {
    if (ts.isExportAssignment(statement) || ts.isExportDeclaration(statement)) {
      return true;
    }
    return isExported(statement) && !ts.isVariableStatement(statement);
  });
}

function isExported(node) {
  return (
    node.modifiers?.some(
      (modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword,
    ) ?? false
  );
}

function scriptKindForPath(filePath) {
  if (filePath.endsWith(".tsx") || filePath.endsWith(".jsx")) {
    return ts.ScriptKind.TSX;
  }
  if (filePath.endsWith(".js") || filePath.endsWith(".mjs")) {
    return ts.ScriptKind.JS;
  }
  return ts.ScriptKind.TS;
}

export { extractExportedConstAssignments, hasUnsupportedExportShape };
