import { unsupportedError } from "./errors.mjs";

function extractCallExpression(source, start, filePath) {
  let index = start;
  while (index < source.length && /[A-Za-z_$]/.test(source[index])) {
    index += 1;
  }

  if (source[index] === "<") {
    index = scanBalanced(source, index, "<", ">", filePath);
  }

  while (index < source.length && /\s/.test(source[index])) {
    index += 1;
  }
  if (source[index] !== "(") {
    throw unsupportedError(filePath, "supported function wrapper call");
  }

  const end = scanBalanced(source, index, "(", ")", filePath);
  return source.slice(start, end);
}

function findCallOpenParen(callExpression, index, filePath) {
  let cursor = index;
  if (callExpression[cursor] === "<") {
    cursor = scanBalanced(callExpression, cursor, "<", ">", filePath);
  }
  while (cursor < callExpression.length && /\s/.test(callExpression[cursor])) {
    cursor += 1;
  }
  if (callExpression[cursor] !== "(") {
    throw unsupportedError(filePath, "function wrapper body");
  }
  return cursor;
}

function parseStringLiteral(text, filePath) {
  const quote = text[0];
  if ((quote !== '"' && quote !== "'") || text[text.length - 1] !== quote) {
    throw unsupportedError(filePath, "non-literal function name");
  }
  return text.slice(1, -1);
}

function findTopLevelColon(text, filePath) {
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let quote = null;

  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (quote) {
      if (character === "\\") {
        index += 1;
        continue;
      }
      if (character === quote) {
        quote = null;
      }
      continue;
    }

    if (character === '"' || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    if (character === "(") {
      parenDepth += 1;
      continue;
    }
    if (character === ")") {
      parenDepth -= 1;
      continue;
    }
    if (character === "[") {
      bracketDepth += 1;
      continue;
    }
    if (character === "]") {
      bracketDepth -= 1;
      continue;
    }
    if (character === "{") {
      braceDepth += 1;
      continue;
    }
    if (character === "}") {
      braceDepth -= 1;
      continue;
    }

    if (
      character === ":" &&
      parenDepth === 0 &&
      bracketDepth === 0 &&
      braceDepth === 0
    ) {
      return index;
    }
  }

  throw unsupportedError(filePath, "object property syntax");
}

function splitTopLevel(text, delimiter, filePath) {
  const parts = [];
  let start = 0;
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let quote = null;

  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (quote) {
      if (character === "\\") {
        index += 1;
        continue;
      }
      if (character === quote) {
        quote = null;
      }
      continue;
    }

    if (character === '"' || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    if (character === "(") {
      parenDepth += 1;
      continue;
    }
    if (character === ")") {
      parenDepth -= 1;
      continue;
    }
    if (character === "[") {
      bracketDepth += 1;
      continue;
    }
    if (character === "]") {
      bracketDepth -= 1;
      continue;
    }
    if (character === "{") {
      braceDepth += 1;
      continue;
    }
    if (character === "}") {
      braceDepth -= 1;
      continue;
    }

    if (
      character === delimiter &&
      parenDepth === 0 &&
      bracketDepth === 0 &&
      braceDepth === 0
    ) {
      parts.push(text.slice(start, index).trim());
      start = index + 1;
    }
  }

  if (quote || parenDepth !== 0 || bracketDepth !== 0 || braceDepth !== 0) {
    throw unsupportedError(filePath, "balanced resolver syntax");
  }

  const tail = text.slice(start).trim();
  if (tail.length > 0) {
    parts.push(tail);
  }
  return parts;
}

function scanBalanced(source, start, open, close, filePath) {
  let depth = 0;
  let quote = null;

  for (let index = start; index < source.length; index += 1) {
    const character = source[index];
    if (quote) {
      if (character === "\\") {
        index += 1;
        continue;
      }
      if (character === quote) {
        quote = null;
      }
      continue;
    }

    if (character === '"' || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    if (character === open) {
      depth += 1;
      continue;
    }
    if (character === close) {
      depth -= 1;
      if (depth === 0) {
        return index + 1;
      }
      continue;
    }
  }

  throw unsupportedError(filePath, "balanced delimiter");
}

function stripQuotes(text) {
  if (
    (text.startsWith('"') && text.endsWith('"')) ||
    (text.startsWith("'") && text.endsWith("'"))
  ) {
    return text.slice(1, -1);
  }
  return text;
}

export {
  extractCallExpression,
  findCallOpenParen,
  findTopLevelColon,
  parseStringLiteral,
  scanBalanced,
  splitTopLevel,
  stripQuotes,
};
