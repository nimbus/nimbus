/// <reference types="node" />
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * Guard for the raw-token / Tailwind-key split in globals.css.
 *
 * The palette is declared twice: once as raw `--nimbus-*` variables and once
 * as the `--color-*` keys inside `@theme inline`. Only the second half is a
 * Tailwind colour key, so only the second half produces utilities. Four raw
 * names are deliberately bridged under a different key — `bg` -> `canvas`,
 * `border` -> `app`, `border-strong` -> `strong`, `text` -> `default` — and
 * writing the raw name into a class instead (`bg-bg`) yields no CSS at all.
 * Tailwind does not warn: the class is simply absent from the stylesheet and
 * the element paints transparent.
 *
 * That is not hypothetical. `bg-bg` shipped on the Slideover panel, so the
 * storage insert and edit drawers rendered with no fill over their scrim.
 *
 * The check only judges suffixes it already knows are palette names, so a
 * size or side utility (`text-sm`, `border-l`) is never a candidate and the
 * scan needs no Tailwind vocabulary of its own.
 */

const SRC = join(dirname(fileURLToPath(import.meta.url)), "..");

/* Read from disk, not imported: `css: false` stubs every CSS module to an
   empty string. Same reason as contrast.spec.ts. */
const CSS = readFileSync(join(SRC, "styles/globals.css"), "utf8");

const bridged = new Set(
  [...CSS.matchAll(/--color-([a-z0-9-]+)\s*:/g)].map((m) => m[1]),
);
const raw = new Set(
  [...CSS.matchAll(/--nimbus-([a-z0-9-]+)\s*:/g)].map((m) => m[1]),
);

/** Prefixes whose value is a colour key when it is a colour at all. */
const PREFIXES =
  /\b(accent|bg|border|caret|decoration|divide|fill|outline|ring|shadow|stroke|text)-([a-z][a-z0-9-]*)\b/g;

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sources(path);
    return entry.name.endsWith(".tsx") ? [path] : [];
  });
}

describe("colour utilities name a registered Tailwind key", () => {
  it("has both halves of the palette to compare", () => {
    expect(bridged.size).toBeGreaterThan(0);
    expect(raw.size).toBeGreaterThan(0);
  });

  it("never names a raw token that @theme does not bridge", () => {
    const dead: string[] = [];
    for (const file of sources(SRC)) {
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(PREFIXES)) {
        const [, prefix, suffix] = match;
        if (!raw.has(suffix) || bridged.has(suffix)) continue;
        const line = text.slice(0, match.index).split("\n").length;
        dead.push(`${relative(SRC, file)}:${line} — ${prefix}-${suffix}`);
      }
    }
    // Named rather than counted, so a failure says which element paints
    // nothing and where.
    expect(dead).toEqual([]);
  });
});
