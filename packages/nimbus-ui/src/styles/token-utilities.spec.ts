/// <reference types="node" />
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * Guard for the role-token vocabulary in class strings.
 *
 * tokens.css declares the palette twice: once as raw `--bg-panel` style
 * variables and once as the `--color-*` keys inside `@theme inline`. Only the
 * second half is a Tailwind colour key, so only the second half produces
 * utilities. The utility spells the whole key after its prefix
 * (`bg-bg-panel`, `text-text-3`, `border-border-2`), and writing the tail
 * alone (`bg-panel`, `text-3`) yields no CSS at all. Tailwind does not warn:
 * the class is absent from the stylesheet and the element paints transparent.
 *
 * That is not hypothetical. `bg-bg` shipped on the Slideover panel under the
 * previous token set, so the storage drawers rendered with no fill over their
 * scrim. The same guard also refuses the retired names from that set, so a
 * revert or a stale branch cannot bring `text-muted` or `bg-surface-2` back.
 */

const SRC = join(dirname(fileURLToPath(import.meta.url)), "..");

/* Read from disk, not imported: `css: false` stubs every CSS module to an
   empty string. Same reason as contrast.spec.ts. */
const TOKENS = readFileSync(join(SRC, "styles/tokens.css"), "utf8");

const bridged = new Set(
  [...TOKENS.matchAll(/--color-([a-z0-9-]+)\s*:/g)].map((m) => m[1]),
);

/** Colour keys of the retired `--nimbus-*` palette. None may return. */
const RETIRED = new Set([
  "canvas",
  "surface",
  "surface-2",
  "app",
  "strong",
  "default",
  "muted",
  "brand",
  "on-brand",
  "focus",
  "link",
  "danger",
  "running",
  "starting",
  "draining",
  "queued",
  "stale",
  "violet",
]);

/** Tails of a role token spelled without its head. */
const TAIL_ONLY = /^(?:canvas|panel|raised|hover|[1-4])$/;

/** Prefixes whose value is a colour key when it is a colour at all. */
const PREFIXES =
  /(?<![A-Za-z0-9-])(accent|bg|border(?:-[xytrblse])?|caret|decoration|divide|fill|outline|ring|shadow|stroke|text|from|via|to)-([a-z][a-z0-9-]*)(?![A-Za-z0-9-])/g;

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      // Registry primitives are CLI-owned and speak the shadcn bridge.
      if (entry.name === "ui" && dir.endsWith("components")) return [];
      return sources(path);
    }
    return entry.name.endsWith(".tsx") && !entry.name.includes(".spec.")
      ? [path]
      : [];
  });
}

describe("colour utilities name a registered Tailwind key", () => {
  it("has the bridge to compare against", () => {
    expect(bridged.has("bg-panel")).toBe(true);
    expect(bridged.has("text-3")).toBe(true);
    expect(bridged.has("accent")).toBe(true);
  });

  it("never names a tail-only or retired token", () => {
    const dead: string[] = [];
    for (const file of sources(SRC)) {
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(PREFIXES)) {
        const [, prefix, suffix] = match;
        const retired = RETIRED.has(suffix);
        // `text-1` is the tail of `text-text-1`; `bg-panel` the tail of
        // `bg-bg-panel`. `border-1` is not a width in this scale, either.
        const tailOnly = TAIL_ONLY.test(suffix);
        if (!retired && !tailOnly) continue;
        const line = text.slice(0, match.index).split("\n").length;
        dead.push(`${relative(SRC, file)}:${line} — ${prefix}-${suffix}`);
      }
    }
    // Named rather than counted, so a failure says which element paints
    // nothing and where.
    expect(dead).toEqual([]);
  });

  it("never reads a retired raw token", () => {
    const stale: string[] = [];
    for (const file of sources(SRC)) {
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(/--nimbus-[a-z0-9-]+/g)) {
        const line = text.slice(0, match.index).split("\n").length;
        stale.push(`${relative(SRC, file)}:${line} — ${match[0]}`);
      }
    }
    expect(stale).toEqual([]);
  });
});

/* The radius scale is `--radius-*: initial` plus six named steps (xs, sm,
   md, lg, xl, full), so Tailwind's bare `rounded` (its DEFAULT step) and
   the `2xl`+ steps compile to nothing. A class that paints nothing is worse
   than a missing one: it reads as intent. Same for the type scale, which
   stops at `2xl`. */
const DEAD_STEPS =
  /(?<![A-Za-z0-9_-])(?:rounded|rounded-(?:[3-9]xl|xxl)|text-(?:[3-9]xl|xxl))(?![A-Za-z0-9_-])/g;

describe("shape and type utilities name a step in the scale", () => {
  it("never uses bare `rounded` or a step outside the scale", () => {
    const dead: string[] = [];
    for (const file of sources(SRC)) {
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(DEAD_STEPS)) {
        const line = text.slice(0, match.index).split("\n").length;
        dead.push(`${relative(SRC, file)}:${line} — ${match[0]}`);
      }
    }
    expect(dead).toEqual([]);
  });
});

/* Labels are sentence case in the sans at `text-xs font-medium`. The old
   console set every label in tracked small caps; that convention is retired
   with the tokens, and the sidebar is the one place a group heading may keep
   it (DESIGN.md §Typography). */
const TRACKED_CAPS =
  /(?<![A-Za-z0-9_-])(?:uppercase|tracking-(?:wider|widest|\[[^\]]+\]))(?![A-Za-z0-9_-])/g;

describe("labels are sentence case", () => {
  it("never sets tracked capitals outside the sidebar", () => {
    const caps: string[] = [];
    for (const file of sources(SRC)) {
      if (file.includes(`${sep}shell${sep}sidebar${sep}`)) continue;
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(TRACKED_CAPS)) {
        const line = text.slice(0, match.index).split("\n").length;
        caps.push(`${relative(SRC, file)}:${line} — ${match[0]}`);
      }
    }
    expect(caps).toEqual([]);
  });
});
