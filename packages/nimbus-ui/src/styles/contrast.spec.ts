/// <reference types="node" />
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * Colour-contrast gate for the palette tokens.
 *
 * The tokens are read out of globals.css rather than duplicated here, so this
 * tracks every future edit instead of drifting from it. There is no browser to
 * resolve the cascade — vitest runs on a synthetic DOM with `css: false` — so
 * the resolution below mirrors the source order and specificity of the
 * `@layer base` blocks by hand.
 *
 * There is no colour library in package.json, so OKLCH -> linear sRGB -> WCAG
 * relative luminance is implemented inline (Ottosson's oklab matrices, CSS
 * Color 4, WCAG 2.x). Out-of-gamut components are clipped. Spot-checked
 * against Chrome: every token in all five palette/mode combinations paints the
 * same sRGB value this produces, give or take 1/255 on one gamut-mapped token.
 */

/* Read from disk, not imported: `css: false` stubs every CSS module to an
   empty string, and Vite rewrites `new URL(…, import.meta.url)` into an asset
   URL rather than a filesystem path. */
const CSS = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "globals.css"),
  "utf8",
);

// --- colour maths -----------------------------------------------------------

function oklchToLinearSrgb(value: string): [number, number, number] {
  const m = /^oklch\(\s*([\d.]+)%\s+([\d.]+)\s+([\d.]+)\s*\)$/i.exec(value);
  if (!m) throw new Error(`expected an oklch() literal, got: ${value}`);
  const L = Number(m[1]) / 100;
  const C = Number(m[2]);
  const h = (Number(m[3]) * Math.PI) / 180;
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);
  // Long, medium and short cone responses, cubed back out of oklab.
  const long = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const med = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const short = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * long - 3.3077115913 * med + 0.2309699292 * short,
    -1.2684380046 * long + 2.6097574011 * med - 0.3413193965 * short,
    -0.0041960863 * long - 0.7034186147 * med + 1.707614701 * short,
  ];
}

function luminance(value: string): number {
  const [r, g, b] = oklchToLinearSrgb(value).map((c) =>
    Math.min(1, Math.max(0, c)),
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [la, lb] = [luminance(a), luminance(b)];
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

// --- cascade resolution -----------------------------------------------------

type Tokens = Record<string, string>;

function block(selector: string): Tokens {
  // Each palette block is a flat declaration list inside `@layer base`.
  const start = CSS.indexOf(`\n  ${selector} {`);
  if (start === -1) throw new Error(`no rule for selector: ${selector}`);
  const open = CSS.indexOf("{", start);
  const close = CSS.indexOf("\n  }", open);
  const body = CSS.slice(open + 1, close);
  const tokens: Tokens = {};
  for (const [, name, value] of body.matchAll(
    /(--nimbus-[a-z0-9-]+)\s*:\s*([^;]+);/g,
  )) {
    tokens[name] = value.trim();
  }
  return tokens;
}

const ROOT = block(":root");
const BLUE = block('[data-palette="blue"]');
const DARK = block('[data-theme="dark"]');
const MONO = block('[data-palette="mono"]');
const MONO_DARK = block('[data-palette="mono"][data-theme="dark"]');

/** Flattens a cascade, then substitutes `var(--nimbus-*)` self-references. */
function resolve(...layers: Tokens[]): Tokens {
  const merged: Tokens = Object.assign({}, ...layers);
  for (let pass = 0; pass < 4; pass++) {
    let changed = false;
    for (const [name, value] of Object.entries(merged)) {
      const next = value.replace(/var\((--nimbus-[a-z0-9-]+)\)/g, (all, ref) =>
        merged[ref] && !merged[ref].includes("var(") ? merged[ref] : all,
      );
      if (next !== value) {
        merged[name] = next;
        changed = true;
      }
    }
    if (!changed) break;
  }
  return merged;
}

// Later layers win: equal-specificity blocks resolve by source order, and the
// two-attribute mono-dark selector outranks both single-attribute blocks.
const PALETTES = {
  "warm light": resolve(ROOT),
  "blue light": resolve(ROOT, BLUE),
  "mono light": resolve(ROOT, MONO),
  "warm dark": resolve(ROOT, DARK),
  "blue dark": resolve(ROOT, BLUE, DARK),
  "mono dark": resolve(ROOT, MONO, DARK, MONO_DARK),
} satisfies Record<string, Tokens>;

const COMBOS = Object.entries(PALETTES);
const BACKDROPS = ["--nimbus-bg", "--nimbus-surface", "--nimbus-surface-2"];

describe("palette token contrast", () => {
  // Every link in the console renders at 11-12px, which is normal text under
  // WCAG, so 4.5:1 is the binding floor on every surface it can land on.
  it.each(COMBOS)("%s: --link clears AA on all three backdrops", (_, t) => {
    for (const backdrop of BACKDROPS) {
      expect(contrast(t["--nimbus-link"], t[backdrop])).toBeGreaterThanOrEqual(
        4.5,
      );
    }
  });

  // Every backdrop, not just --surface. The check used to read "these are
  // painted on panels, tables and popovers, i.e. on --surface", and --surface
  // in warm light is pure white, which is the most forgiving ground in the
  // console. Nothing stops a call site from putting the same token on the
  // canvas, and /operator/settings does: its unavailable-section text is
  // --danger on --bg, where a browser measured 4.3:1 while this gate reported
  // a pass against white. A semantic text token has to clear AA everywhere it
  // can land, which is what the --link check above already assumed.
  it.each(COMBOS)("%s: text tokens clear AA on every backdrop", (_, t) => {
    const short: string[] = [];
    for (const token of [
      "--nimbus-muted",
      "--nimbus-warning",
      "--nimbus-success",
      "--nimbus-danger",
      "--nimbus-link",
    ]) {
      for (const backdrop of BACKDROPS) {
        const ratio = contrast(t[token], t[backdrop]);
        if (ratio < 4.5) {
          short.push(`${token} on ${backdrop} — ${ratio.toFixed(2)}:1`);
        }
      }
    }
    // Named rather than counted: a failure has to say which token is illegible
    // on which ground, or the next person re-derives it by hand.
    expect(short).toEqual([]);
  });

  // WCAG 2.2 SC 1.4.11: 3:1 for focus indicators and meaningful graphics.
  it.each(COMBOS)("%s: --focus and --running clear the 3:1 floor", (_, t) => {
    for (const token of ["--nimbus-focus", "--nimbus-running"]) {
      for (const backdrop of BACKDROPS) {
        expect(contrast(t[token], t[backdrop])).toBeGreaterThanOrEqual(3);
      }
    }
  });

  // --accent once equalled --muted byte-for-byte in mono light, which made the
  // focus ring, the selection fill and every accent-bound state the same grey
  // as secondary text.
  it.each(COMBOS)("%s: --accent is distinguishable from --muted", (_, t) => {
    expect(t["--nimbus-accent"]).not.toBe(t["--nimbus-muted"]);
    expect(contrast(t["--nimbus-accent"], t["--nimbus-muted"])).toBeGreaterThan(
      1.4,
    );
  });

  // Running is a state, not an identity colour: it must not collapse into the
  // "no state" grey, and it must stay separable from the other health states.
  it.each(COMBOS)("%s: --running stays distinct from other states", (_, t) => {
    for (const other of ["--nimbus-muted", "--nimbus-success"]) {
      expect(t["--nimbus-running"]).not.toBe(t[other]);
    }
  });
});

describe("palette identity", () => {
  it("warm and blue resolve to the same Night Blue dark theme", () => {
    // DESIGN.md: "Dark mode is Night Blue for every palette except mono."
    // The appearance swatch previews this, so it must stay true.
    expect(PALETTES["blue dark"]).toEqual(PALETTES["warm dark"]);
    expect(PALETTES["mono dark"]).not.toEqual(PALETTES["warm dark"]);
  });

  it("the three light palettes stay visually distinct", () => {
    const identity = (t: Tokens) =>
      [t["--nimbus-brand"], t["--nimbus-accent"], t["--nimbus-link"]].join("|");
    const seen = ["warm light", "blue light", "mono light"].map((k) =>
      identity(PALETTES[k as keyof typeof PALETTES]),
    );
    expect(new Set(seen).size).toBe(3);
  });

  it("a palette whose --link matches --text carries an underlined link class", () => {
    // Mono is deliberately chromaless, so its links cannot be identified by
    // hue (WCAG 1.4.1). The underline has to do that work instead.
    const chromaless = COMBOS.filter(
      ([, t]) => t["--nimbus-link"] === t["--nimbus-text"],
    );
    const names = chromaless.map(([name]) => name);
    expect(names).toEqual(["mono light", "mono dark"]);

    const rule = /\.link-inline\s*\{[^}]*\}/.exec(CSS)?.[0] ?? "";
    expect(rule).toMatch(/text-decoration:\s*underline/);
    expect(rule).toMatch(/color:\s*var\(--nimbus-link\)/);
  });
});

const REDUCED_MOTION =
  /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\n {2}\}/;

describe("motion", () => {
  it("guards every animation and transition behind prefers-reduced-motion", () => {
    const guard = REDUCED_MOTION.exec(CSS)?.[0];
    expect(guard).toBeTruthy();
    expect(guard).toMatch(/animation-duration:\s*0\.01ms\s*!important/);
    expect(guard).toMatch(/transition-duration:\s*0\.01ms\s*!important/);
  });
});
