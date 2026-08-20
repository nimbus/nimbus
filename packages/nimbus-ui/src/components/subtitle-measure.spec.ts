/// <reference types="node" />
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * Length budget for `PageHeader` subtitle prose.
 *
 * The console used to cap the subtitle in CSS (`max-w-[68ch]`), which was the
 * wrong instrument twice over. `1ch` is the advance of "0" — 7.55px in this
 * font against a ~5.7px average prose advance — so "68ch" set a 90-character
 * measure, not 68. And the paragraph is shrink-to-fit inside a header row, so
 * it never grew past its own sentence: the cap could only ever add a line,
 * never remove one. It added one on 12 of 23 routes at 1280, 1440 and 1920
 * alike, which stepped the content panel down by a line-height as you moved
 * between pages.
 *
 * An over-long subtitle is a copy problem, so the constraint belongs on the
 * copy, where the author writing it can see it. 100 characters sets one line
 * on every route at 1440px and wider.
 *
 * Prose this scan cannot resolve — a subtitle assembled at runtime — is
 * skipped here and left to the `max-w-[110ch]` runaway guard on the
 * component, which is why that guard is not redundant with this test.
 */
const BUDGET = 100;

const SRC = join(dirname(fileURLToPath(import.meta.url)), "..");

/** A reference like `SECTION_SUBTITLES[section]`, whose text lives elsewhere. */
const REFERENCE = /^[A-Za-z_$][\w$.]*(?:\[[^\]]*\])?$/;

type Shape = "string" | "expression" | "map";
type Subtitle = { file: string; line: number; shape: Shape; prose: string };

/** Slice a `{...}` expression, ignoring delimiters inside string literals. */
function braced(text: string, open: number): string {
  let depth = 0;
  let quote = "";
  for (let i = open; i < text.length; i += 1) {
    const c = text[i];
    if (quote) {
      if (c === "\\") i += 1;
      else if (c === quote) quote = "";
      continue;
    }
    if (c === '"' || c === "'" || c === "`") quote = c;
    else if (c === "{") depth += 1;
    else if (c === "}") {
      depth -= 1;
      if (depth === 0) return text.slice(open + 1, i);
    }
  }
  return "";
}

function quoted(text: string, open: number): string {
  for (let i = open + 1; i < text.length; i += 1) {
    if (text[i] === "\\") i += 1;
    else if (text[i] === text[open]) return text.slice(open + 1, i);
  }
  return "";
}

/**
 * Drop JSX tags and keep the text between them. Quote-aware for the same
 * reason `braced` above is: `>` is legal inside an attribute value, and a
 * pattern that stops at the first one leaves the tail of the attribute behind
 * as text -- including its closing quote, which the literal sweep below would
 * then pair with the next quote in the expression and read a fragment of
 * markup as copy. A `<` with no tag end after it is a comparison, not a tag,
 * and stays.
 */
function untagged(text: string): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    if (text[i] !== "<") {
      out += text[i];
      i += 1;
      continue;
    }
    const end = tagEnd(text, i);
    if (end === -1) {
      out += text[i];
      i += 1;
      continue;
    }
    i = end + 1;
  }
  return out;
}

/** Index of the `>` closing the tag opened at `open`, or -1 if there is none. */
function tagEnd(text: string, open: number): number {
  let quote = "";
  for (let i = open + 1; i < text.length; i += 1) {
    const c = text[i];
    if (quote) {
      if (c === "\\") i += 1;
      else if (c === quote) quote = "";
      continue;
    }
    if (c === '"' || c === "'" || c === "`") quote = c;
    else if (c === ">") return i;
  }
  return -1;
}

/**
 * Reduce a subtitle expression to the strings a reader actually sees. A
 * conditional yields one per branch; a JSX fragment yields its flattened text.
 */
function prose(expression: string): string[] {
  const spaced = expression.replace(/\{"\s*"\}/g, " ");
  // Tags come out before literals are swept, so a `className` value is never
  // mistaken for copy.
  const stripped = untagged(spaced);
  const literals = [...stripped.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g)]
    .map((match) => match[1])
    .filter((text) => text.trim() !== "");
  if (literals.length > 0) return literals;
  const text = stripped.replace(/\s+/g, " ").trim();
  return text === "" || REFERENCE.test(text) ? [] : [text];
}

function lineOf(text: string, index: number): number {
  return text.slice(0, index).split("\n").length;
}

function subtitlesIn(file: string, text: string): Subtitle[] {
  const found: Subtitle[] = [];
  const add = (index: number, shape: Shape, values: string[]) => {
    for (const value of values) {
      found.push({ file, line: lineOf(text, index), shape, prose: value });
    }
  };

  for (const match of text.matchAll(/\bsubtitle=/g)) {
    const at = (match.index ?? 0) + match[0].length;
    if (text[at] === '"') add(match.index ?? 0, "string", [quoted(text, at)]);
    else if (text[at] === "{")
      add(match.index ?? 0, "expression", prose(braced(text, at)));
  }

  // The console's one indirection: Settings indexes a map of subtitles rather
  // than writing them at the call site.
  for (const match of text.matchAll(/const\s+\w*SUBTITLES\w*\b[^=]*=\s*\{/g)) {
    const open = (match.index ?? 0) + match[0].length - 1;
    add(match.index ?? 0, "map", prose(braced(text, open)));
  }

  return found;
}

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sources(path);
    if (entry.name.includes(".spec.")) return [];
    return entry.name.endsWith(".tsx") ? [path] : [];
  });
}

const found = sources(SRC).flatMap((path) =>
  subtitlesIn(relative(SRC, path), readFileSync(path, "utf8")),
);

describe("page subtitles fit one line", () => {
  // Without this, a regex that stopped matching would report a clean sweep of
  // nothing at all.
  it("reads every shape a subtitle is written in", () => {
    expect(found.length).toBeGreaterThanOrEqual(20);
    expect([...new Set(found.map((s) => s.shape))].sort()).toEqual([
      "expression",
      "map",
      "string",
    ]);
  });

  it("reads copy out of a tag that carries a `>` in an attribute", () => {
    // The tail of `title` used to survive the strip, and its closing quote
    // paired with the next one to make `" and ` look like the copy.
    expect(prose('<span title="a > b">Real copy</span>')).toEqual([
      "Real copy",
    ]);
    expect(prose('<b className="x">"Quoted copy"</b>')).toEqual([
      "Quoted copy",
    ]);
  });

  it("keeps a comparison that never opens a tag", () => {
    expect(prose('count < limit ? "Under" : "Over"')).toEqual([
      "Under",
      "Over",
    ]);
  });

  it("keeps every subtitle inside the budget", () => {
    // Named and measured rather than counted, so a failure says which page to
    // edit and by how much.
    const over = found
      .filter((s) => s.prose.length > BUDGET)
      .map((s) => `${s.file}:${s.line} — ${s.prose.length} chars — ${s.prose}`);
    expect(over).toEqual([]);
  });
});
