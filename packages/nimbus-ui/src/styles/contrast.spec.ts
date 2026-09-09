/// <reference types="node" />
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * Colour-contrast and focus-indicator gates for the token sheet.
 *
 * The tokens are read from styles/tokens.css on disk, not imported: vitest
 * runs with `css: false`, which stubs every CSS module to an empty string.
 * Every token value is a hex or rgba literal, which is what keeps this file
 * a parser rather than a colour engine.
 */

const STYLES = dirname(fileURLToPath(import.meta.url));
const SRC = join(STYLES, "..");
const TOKENS = readFileSync(join(STYLES, "tokens.css"), "utf8");
const GLOBALS = readFileSync(join(STYLES, "globals.css"), "utf8");

// --- colour maths -----------------------------------------------------------

type Rgb = [number, number, number];

function parseColour(value: string): Rgb {
  const hex = value.match(/^#([0-9a-f]{6})$/i);
  if (hex) {
    const n = parseInt(hex[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  const rgba = value.match(
    /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*[\d.]+\s*)?\)$/,
  );
  if (rgba) return [Number(rgba[1]), Number(rgba[2]), Number(rgba[3])];
  throw new Error(`unsupported colour literal: ${value}`);
}

function luminance([r, g, b]: Rgb): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contrast(fg: string, bg: string): number {
  const a = luminance(parseColour(fg));
  const b = luminance(parseColour(bg));
  const [hi, lo] = a > b ? [a, b] : [b, a];
  return (hi + 0.05) / (lo + 0.05);
}

// --- token parsing ----------------------------------------------------------

type Tokens = Record<string, string>;

/** The declarations of one top-level `<selector> { ... }` block. */
function block(selector: string): Tokens {
  const start = TOKENS.indexOf(`\n${selector} {`);
  if (start < 0) throw new Error(`no block for ${selector}`);
  const end = TOKENS.indexOf("\n}", start);
  const body = TOKENS.slice(start, end);
  const out: Tokens = {};
  for (const m of body.matchAll(/(--[a-z0-9-]+):\s*([^;]+);/g)) {
    out[m[1]] = m[2].trim();
  }
  return out;
}

const THEMES = {
  dark: block(":root"),
  light: { ...block(":root"), ...block(':root[data-theme="light"]') },
} satisfies Record<string, Tokens>;

const COMBOS = Object.entries(THEMES);
const GROUNDS = ["--bg-canvas", "--bg-panel", "--bg-raised", "--bg-hover"];

// Every token that paints text somewhere in the console. Rows, cells, links,
// and states all land on hovered rows and raised panels as often as on the
// canvas, so the floor is checked on every ground, not only the darkest or
// lightest one.
const TEXT_TOKENS = [
  "--text-1",
  "--text-2",
  "--text-3",
  "--accent-link",
  "--success",
  "--warning",
  "--error",
  "--info",
];

describe("role token contrast", () => {
  it("declares the same token set in both themes", () => {
    const light = block(':root[data-theme="light"]');
    const dark = block(":root");
    expect(Object.keys(light).sort()).toEqual(Object.keys(dark).sort());
  });

  // Normal text under WCAG: 4.5:1. Links render at 12-13px, so no large-text
  // relief applies anywhere in the console.
  it.each(COMBOS)("%s: every text token clears AA on every ground", (_, t) => {
    const failures: string[] = [];
    for (const token of TEXT_TOKENS) {
      for (const ground of GROUNDS) {
        const ratio = contrast(t[token], t[ground]);
        if (ratio < 4.5) {
          failures.push(`${token} on ${ground}: ${ratio.toFixed(2)}:1`);
        }
      }
    }
    // Named rather than counted, so a failure says which pair and by how much.
    expect(failures).toEqual([]);
  });

  // --accent paints fills and the focus ring, which are non-text UI
  // components under SC 1.4.11: 3:1 on every ground it can sit on.
  it.each(COMBOS)("%s: --accent clears the 3:1 non-text floor", (_, t) => {
    for (const ground of GROUNDS) {
      expect(contrast(t["--accent"], t[ground])).toBeGreaterThanOrEqual(3);
    }
  });

  // Text on an accent fill (primary buttons, the selected segment).
  it.each(COMBOS)("%s: --accent-ink clears AA on --accent", (_, t) => {
    expect(contrast(t["--accent-ink"], t["--accent"])).toBeGreaterThanOrEqual(
      4.5,
    );
    expect(contrast(t["--error-ink"], t["--error"])).toBeGreaterThanOrEqual(
      4.5,
    );
  });

  // --text-4 is the one documented non-AA token (disabled only, which WCAG
  // exempts). What is checked is its place in the scale: the disabled tier
  // is quieter than the metadata tier, so a disabled control never reads as
  // live metadata.
  it.each(COMBOS)("%s: --text-4 sits below --text-3 in the scale", (_, t) => {
    expect(contrast(t["--text-4"], t["--bg-canvas"])).toBeLessThan(
      contrast(t["--text-3"], t["--bg-canvas"]),
    );
  });

  // Semantic colours never use the accent hue, and a state never collapses
  // into the "no state" grey: the four state tokens, the accent and text-3
  // are six distinct literals in both themes.
  it.each(COMBOS)("%s: states, accent and text-3 are distinct", (_, t) => {
    const values = [
      t["--success"],
      t["--warning"],
      t["--error"],
      t["--info"],
      t["--accent"],
      t["--text-3"],
    ];
    expect(new Set(values).size).toBe(values.length);
  });

  // The four grounds step monotonically so a raised panel reads as raised.
  it.each(COMBOS)("%s: the grounds step in one direction", (name, t) => {
    const l = GROUNDS.map((g) => luminance(parseColour(t[g])));
    for (let i = 1; i < l.length; i += 1) {
      if (name === "dark") expect(l[i]).toBeGreaterThan(l[i - 1]);
      else expect(l[i]).toBeLessThan(l[i - 1]);
    }
  });
});

// --- focus indicators -------------------------------------------------------

/* One rule paints every focus ring: the unlayered `:focus-visible` in
   globals.css. Tailwind emits utilities inside `@layer utilities` and an
   unlayered declaration beats every layered one, so this rule also recolours
   the shadcn registry primitives, which keep `focus-visible:ring-ring/50` as
   written under components/ui.

   Two things keep that true. The rule itself must stay unlayered and must
   name `--accent` (the ring token, DESIGN.md accent job 3). And no Nimbus-owned
   component may bind a ring or outline colour of its own under a focus
   variant: that would either duplicate the global ring or, on a token that
   misses 3:1, paint a worse one. */

function unlayeredRules(css: string): string[] {
  const rules: string[] = [];
  let depth = 0;
  let start = -1;
  for (let i = 0; i < css.length; i += 1) {
    const ch = css[i];
    if (ch === "{") {
      if (depth === 0) start = css.lastIndexOf("\n", i) + 1;
      depth += 1;
    } else if (ch === "}") {
      depth -= 1;
      if (depth === 0 && start >= 0) {
        const rule = css.slice(start, i + 1);
        if (!rule.startsWith("@")) rules.push(rule);
        start = -1;
      }
    }
  }
  return rules;
}

/* A ring or outline colour under a focus variant is a second focus ring by
   construction. A border colour under a focus variant is allowed on one
   token only: `border-accent`, the accent border a text field shows together
   with the ring (DESIGN.md inputs). Any other focus border is a private
   indicator on a token that was never measured for the job. */
const RING_COLOUR =
  /\b(?:focus|focus-visible|focus-within):(?:ring|outline)-(?:\[color:var\((--[a-z0-9-]+)\)\]|(?:accent|accent-link|success|warning|error|info|text-[1-4]|border-[1-3]|ring|destructive|bg-[a-z]+))(?:\/\d+)?(?![\w-])/g;
const BORDER_COLOUR =
  /\b(?:focus|focus-visible|focus-within):border-(?:\[color:var\((--[a-z0-9-]+)\)\]|(?:accent-link|success|warning|error|info|text-[1-4]|border-[1-3]|ring|destructive|bg-[a-z]+))(?:\/\d+)?(?![\w-])/g;

function tsxFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "ui" && dir.endsWith("components")) continue;
      out.push(...tsxFiles(path));
    } else if (entry.name.endsWith(".tsx") && !entry.name.includes(".spec.")) {
      out.push(path);
    }
  }
  return out;
}

describe("focus indicators", () => {
  it("paints the global ring from --accent in one unlayered rule", () => {
    const rule = unlayeredRules(GLOBALS).find((r) =>
      r.startsWith(":focus-visible {"),
    );
    expect(rule).toBeDefined();
    expect(rule).toMatch(/outline:\s*none;/);
    expect(rule).toMatch(/box-shadow:[\s\S]*var\(--accent\)/);
    // Two layers: an inner 2px band and an outer 4px halo.
    expect(rule).toMatch(/0 0 0 2px/);
    expect(rule).toMatch(/0 0 0 4px/);
  });

  it("names no other focus ring token in the stylesheet", () => {
    expect(GLOBALS).not.toMatch(/--tw-ring-color/);
    expect(TOKENS).not.toMatch(/--focus\b/);
  });

  it("binds no private focus ring colour in a Nimbus-owned component", () => {
    const offenders: string[] = [];
    for (const file of tsxFiles(SRC)) {
      const text = readFileSync(file, "utf8");
      for (const pattern of [RING_COLOUR, BORDER_COLOUR]) {
        for (const match of text.matchAll(pattern)) {
          const line = text.slice(0, match.index).split("\n").length;
          offenders.push(`${relative(SRC, file)}:${line} ${match[0]}`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });
});

// --- links ------------------------------------------------------------------

describe("inline links", () => {
  it("identify themselves by a resting underline, not colour alone", () => {
    const rule = GLOBALS.match(/\.link-inline \{([\s\S]*?)\n {2}\}/);
    expect(rule).not.toBeNull();
    expect(rule?.[1]).toMatch(/color:\s*var\(--accent-link\)/);
    expect(rule?.[1]).toMatch(/text-decoration:\s*underline/);
  });
});

// --- motion -----------------------------------------------------------------

const REDUCED_MOTION =
  /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\n {2}\}/;

describe("reduced motion", () => {
  it("collapses every animation and transition under the preference", () => {
    const rule = GLOBALS.match(REDUCED_MOTION)?.[0];
    expect(rule).toBeDefined();
    expect(rule).toMatch(/animation-duration:\s*0\.01ms !important/);
    expect(rule).toMatch(/transition-duration:\s*0\.01ms !important/);
  });
});
