import fs from "node:fs";
import path from "node:path";

export function maskNonCode(sourceText) {
  const lexicalView = sourceText.split("");
  const blank = (start, end) => {
    for (let cursor = start; cursor < end; cursor += 1) {
      if (lexicalView.at(cursor) !== "\n" && lexicalView.at(cursor) !== "\r") {
        lexicalView.splice(cursor, 1, " ");
      }
    }
  };

  let cursor = 0;
  while (cursor < sourceText.length) {
    if (sourceText.startsWith("//", cursor)) {
      const end = sourceText.indexOf("\n", cursor + 2);
      blank(cursor, end < 0 ? sourceText.length : end);
      cursor = end < 0 ? sourceText.length : end;
      continue;
    }
    if (sourceText.startsWith("/*", cursor)) {
      let depth = 1;
      let end = cursor + 2;
      while (end < sourceText.length && depth > 0) {
        if (sourceText.startsWith("/*", end)) {
          depth += 1;
          end += 2;
        } else if (sourceText.startsWith("*/", end)) {
          depth -= 1;
          end += 2;
        } else {
          end += 1;
        }
      }
      blank(cursor, end);
      cursor = end;
      continue;
    }

    const raw = sourceText.slice(cursor).match(/^(?:br|rb|cr|r)(#*)"/);
    if (raw) {
      const terminator = `"${raw[1]}`;
      const contentStart = cursor + raw[0].length;
      const found = sourceText.indexOf(terminator, contentStart);
      const end = found < 0 ? sourceText.length : found + terminator.length;
      blank(cursor, end);
      cursor = end;
      continue;
    }

    const quoteOffset =
      ["b", "c"].includes(sourceText[cursor]) && sourceText[cursor + 1] === '"'
        ? 1
        : 0;
    if (sourceText[cursor + quoteOffset] === '"') {
      let end = cursor + quoteOffset + 1;
      while (end < sourceText.length) {
        if (sourceText[end] === "\\") {
          end += 2;
        } else if (sourceText[end] === '"') {
          end += 1;
          break;
        } else {
          end += 1;
        }
      }
      blank(cursor, end);
      cursor = end;
      continue;
    }

    if (sourceText[cursor] === "'") {
      const character = sourceText
        .slice(cursor)
        .match(/^'(?:\\.|[^\\'\r\n])'/u);
      if (character) {
        blank(cursor, cursor + character[0].length);
        cursor += character[0].length;
        continue;
      }
    }
    cursor += 1;
  }
  return lexicalView.join("");
}

export function withoutCfgTestItems(sourceText) {
  const lexicalView = maskNonCode(sourceText);
  const ranges = [];
  const cfgTest = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g;
  let attribute;
  while ((attribute = cfgTest.exec(lexicalView)) !== null) {
    if (
      ranges.some(
        ([start, end]) => attribute.index >= start && attribute.index < end,
      )
    ) {
      continue;
    }
    let cursor = cfgTest.lastIndex;
    let parentheses = 0;
    let brackets = 0;
    let itemEnd = -1;
    while (cursor < lexicalView.length) {
      const token = lexicalView.at(cursor);
      if (token === "(") parentheses += 1;
      else if (token === ")") parentheses = Math.max(0, parentheses - 1);
      else if (token === "[") brackets += 1;
      else if (token === "]") brackets = Math.max(0, brackets - 1);
      else if (parentheses === 0 && brackets === 0 && token === ";") {
        itemEnd = cursor + 1;
        break;
      } else if (parentheses === 0 && brackets === 0 && token === "{") {
        let depth = 1;
        cursor += 1;
        while (cursor < lexicalView.length && depth > 0) {
          if (lexicalView.at(cursor) === "{") depth += 1;
          else if (lexicalView.at(cursor) === "}") depth -= 1;
          cursor += 1;
        }
        itemEnd = cursor;
        break;
      }
      cursor += 1;
    }
    ranges.push([attribute.index, itemEnd < 0 ? lexicalView.length : itemEnd]);
  }

  const visible = lexicalView.split("");
  for (const [start, end] of ranges) {
    for (let cursor = start; cursor < end; cursor += 1) {
      if (visible[cursor] !== "\n" && visible[cursor] !== "\r") {
        visible[cursor] = " ";
      }
    }
  }
  return visible.join("");
}

export function walkRust(directory) {
  const sources = [];
  if (!fs.existsSync(directory) || !fs.statSync(directory).isDirectory()) {
    return sources;
  }
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "tests" || entry.name === "benches") continue;
      sources.push(...walkRust(full));
    } else if (
      entry.isFile() &&
      entry.name.endsWith(".rs") &&
      entry.name !== "tests.rs"
    ) {
      sources.push({
        file: full.split(path.sep).join("/"),
        source: withoutCfgTestItems(fs.readFileSync(full, "utf8")),
      });
    }
  }
  return sources;
}

export function addFixture(sources, environmentName) {
  const fixture = process.env[environmentName];
  if (process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1" && fixture) {
    const configuredPath = process.env[`${environmentName}_PATH`];
    sources.push({
      file:
        configuredPath ||
        `__nimbus_network_verifier_self_test__/${environmentName}.rs`,
      source: withoutCfgTestItems(fixture),
    });
  }
}

export function location(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

export function firstMatch(sources, pattern) {
  for (const candidate of sources) {
    const match = candidate.source.match(pattern);
    if (match) {
      return `${candidate.file}:${location(candidate.source, match.index)}:${match[0]
        .replace(/\s+/g, " ")
        .trim()}`;
    }
  }
  return null;
}

export function definitions(sources, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(
    `\\b(?:pub(?:\\s*\\([^)]*\\))?\\s+)?(?:struct|enum|trait|type)\\s+${escaped}\\b`,
    "g",
  );
  const found = [];
  for (const candidate of sources) {
    let match;
    while ((match = pattern.exec(candidate.source)) !== null) {
      found.push(
        `${candidate.file}:${location(candidate.source, match.index)}`,
      );
    }
  }
  return found;
}

export function allMatches(sources, pattern) {
  const found = [];
  for (const candidate of sources) {
    pattern.lastIndex = 0;
    let match;
    while ((match = pattern.exec(candidate.source)) !== null) {
      found.push({
        file: candidate.file,
        line: location(candidate.source, match.index),
        text: match[0].replace(/\s+/g, " ").trim(),
      });
      if (!pattern.global) break;
    }
  }
  return found;
}
