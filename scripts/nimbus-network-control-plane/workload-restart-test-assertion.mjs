// Lexical assertion-shape checks for the NNCV034 attributed Rust tests.
// Rust execution proves behavior. This parser only rejects empty, helper-only,
// comment-only, string-only, and tautological proof bodies.

import { maskNonCode } from "./source-contract-scanner.mjs";

export function createAttributedTestChecker(extractItem) {
  return function hasTestsAt(sources, file, testNames) {
    const source =
      sources.testEntries.find((entry) => entry.file === file)?.source ?? "";
    return testNames.every((name) =>
      hasExecutableTest(source, name, extractItem),
    );
  };
}

export function remaskTestSources(sources) {
  for (const entry of sources.testEntries) {
    entry.source = maskNonCode(entry.source);
  }
  sources.tests = sources.testEntries.map((entry) => entry.source).join("\n");
}

function hasExecutableTest(source, name, extractItem) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  const attributed = new RegExp(
    `#\\s*\\[\\s*(?:(?:tokio|rstest)\\s*::\\s*)?test\\b[^\\]]*\\]\\s*(?:#\\s*\\[[^\\]]*\\]\\s*)*(?:async\\s+)?fn\\s+${escaped}\\b`,
    "mu",
  );
  const match = attributed.exec(source);
  if (!match) return false;
  const functionStart = source.indexOf(`fn ${name}`, match.index);
  const item = extractItem(source.slice(functionStart), `fn ${name}`);
  const bodyStart = item.indexOf("{");
  return (
    bodyStart >= 0 &&
    hasMeaningfulOutcomeAssertion(item.slice(bodyStart + 1, -1))
  );
}

export function hasMeaningfulOutcomeAssertion(body) {
  const assertion =
    /\b(assert|assert_eq|assert_ne|assert_matches|debug_assert|debug_assert_eq|debug_assert_ne)\s*!\s*\(/gu;
  let match;
  while ((match = assertion.exec(body)) !== null) {
    const open = body.indexOf("(", match.index);
    const close = matchingDelimiter(body, open);
    if (close < 0) continue;
    const arguments_ = topLevelArguments(body.slice(open + 1, close));
    const macro = match[1];
    if (macro === "assert" || macro === "debug_assert") {
      if (isMeaningfulPredicate(arguments_[0] ?? "")) return true;
    } else if (macro === "assert_matches") {
      const value = normalizeAssertionExpression(arguments_[0] ?? "");
      const pattern = normalizeAssertionExpression(arguments_[1] ?? "");
      if (value && pattern && pattern !== "_") return true;
    } else if (isMeaningfulComparison(arguments_)) {
      return true;
    }
    assertion.lastIndex = close + 1;
  }
  return false;
}

function matchingDelimiter(source, open) {
  if (open < 0 || source[open] !== "(") return -1;
  const openings = new Map([
    ["(", ")"],
    ["[", "]"],
    ["{", "}"],
  ]);
  const stack = [")"];
  for (let cursor = open + 1; cursor < source.length; cursor += 1) {
    const token = source[cursor];
    if (openings.has(token)) {
      stack.push(openings.get(token));
    } else if (token === stack.at(-1)) {
      stack.pop();
      if (stack.length === 0) return cursor;
    }
  }
  return -1;
}

function topLevelArguments(source) {
  const arguments_ = [];
  const openings = new Map([
    ["(", ")"],
    ["[", "]"],
    ["{", "}"],
  ]);
  const stack = [];
  let start = 0;
  for (let cursor = 0; cursor < source.length; cursor += 1) {
    const token = source[cursor];
    if (openings.has(token)) {
      stack.push(openings.get(token));
    } else if (token === stack.at(-1)) {
      stack.pop();
    } else if (token === "," && stack.length === 0) {
      arguments_.push(source.slice(start, cursor).trim());
      start = cursor + 1;
    }
  }
  arguments_.push(source.slice(start).trim());
  return arguments_;
}

function normalizeAssertionExpression(expression) {
  let normalized = expression.replace(/\s+/gu, "");
  while (
    normalized.startsWith("(") &&
    matchingDelimiter(normalized, 0) === normalized.length - 1
  ) {
    normalized = normalized.slice(1, -1);
  }
  return normalized;
}

function isLiteralAssertionExpression(expression) {
  return /^(?:true|false|[-+]?(?:0[xob])?[0-9A-Fa-f_]+(?:\.[0-9_]+)?(?:[iu](?:8|16|32|64|128|size)|f(?:32|64))?|\(\))$/u.test(
    expression,
  );
}

function isMeaningfulPredicate(expression) {
  const predicate = normalizeAssertionExpression(expression);
  if (!predicate || /^(?:true|false|!true|!false)$/u.test(predicate)) {
    return false;
  }
  const comparison = predicate.match(/^(.+?)(==|!=)(.+)$/u);
  if (!comparison) return true;
  const left = normalizeAssertionExpression(comparison[1]);
  const right = normalizeAssertionExpression(comparison[3]);
  return (
    left !== right &&
    !(isLiteralAssertionExpression(left) && isLiteralAssertionExpression(right))
  );
}

function isMeaningfulComparison(arguments_) {
  if (arguments_.length < 2) return false;
  const left = normalizeAssertionExpression(arguments_[0]);
  const right = normalizeAssertionExpression(arguments_[1]);
  return (
    Boolean(left) &&
    Boolean(right) &&
    left !== right &&
    !(isLiteralAssertionExpression(left) && isLiteralAssertionExpression(right))
  );
}
