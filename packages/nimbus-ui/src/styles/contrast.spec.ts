/// <reference types="node" />
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
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
const HERE = dirname(fileURLToPath(import.meta.url));
const SRC = join(HERE, "..");
const CSS = readFileSync(join(HERE, "globals.css"), "utf8");

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

// --- focus indicators at the call sites -------------------------------------

/* The token table above is only half the contract. It proves `--focus` clears
   the floor; it cannot stop a component binding its ring to a token that does
   not. Nothing here used to, and four call sites had drifted onto tokens that
   fail outright: measured in Chrome, `--accent` is 1.71:1 on `--surface-2` in
   warm light and 2.24:1 in blue, and `--brand` is 2.21:1 and 3.32:1, against
   SC 1.4.11's 3:1 non-text floor.

   They were not sloppy, they were compliant — DESIGN.md's token table gave
   `--accent` the job "focus ring and selection" in writing, and globals.css
   disagreed. The document is being corrected; this gate is what keeps the code
   from drifting back, so it must fail on any NEW binding anywhere in src, not
   just on the four that are fixed.

   Scope: a ring or outline colour on a `focus:`/`focus-visible:` variant. That
   is a focus indicator by construction, so the 3:1 floor is unambiguous. A ring
   with no focus variant is decoration and is out of scope, as is
   `shadow-[inset_2px_0_0_var(--accent)]` — the sanctioned selection bar, which
   is identity rather than focus and may sit below 3:1. */

/* Both spellings reach the same token: `ring-[color:var(--nimbus-accent)]` and
   the `--color-*` utility `ring-accent`. Catching only the first would leave
   the second as an open door, so the utility map is parsed out of `@theme
   inline` — a new `--color-*` is then covered the day it is added. */
const THEME_COLOURS = new Map(
  [
    ...CSS.matchAll(/--color-([a-z0-9-]+):\s*var\((--nimbus-[a-z0-9-]+)\)/g),
  ].map(([, utility, token]) => [utility, token]),
);

const FOCUS_RING =
  /\b(?:focus|focus-visible):(?:ring|outline)-(?:\[color:var\((--nimbus-[a-z0-9-]+)\)\]|([a-z0-9-]+))/g;

function tsxFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...tsxFiles(path));
    else if (entry.name.endsWith(".tsx")) out.push(path);
  }
  return out;
}

describe("focus indicators", () => {
  it("maps the --color-* utilities so both ring spellings are covered", () => {
    // If this map ever comes back empty the scan below still passes while
    // checking nothing, which is the one way this gate could fail silently.
    expect(THEME_COLOURS.get("accent")).toBe("--nimbus-accent");
    expect(THEME_COLOURS.get("brand")).toBe("--nimbus-brand");
    expect(THEME_COLOURS.get("focus")).toBe("--nimbus-focus");
  });

  /* DESIGN.md: "a focus ring names `--focus`, never `--accent` directly."

     That is a naming rule, and it has to be checked as one. Measuring the
     bound colour instead is not equivalent, for two reasons that pull in
     opposite directions.

     `--focus` is declared `var(--accent)` in the dark palettes and in mono
     light, so in four of the six combinations the two tokens resolve to the
     same literal and no measurement can tell them apart. What separates them
     is warm light and blue light, where `--focus` diverges precisely because
     `--accent` cannot carry a ring there.

     And a pure value check is too weak in the other direction: `--text` and
     `--danger` clear 3:1 on every ground, so a ring bound to either would pass
     a threshold gate while still breaking the contract — verified, both did.
     So the token is checked by name, and the measured ratio is carried into
     the message to say why the rule exists rather than merely that it was
     broken. */
  it("name --focus, never another token that measures well", () => {
    const failures: string[] = [];
    for (const file of tsxFiles(SRC)) {
      const name = relative(SRC, file).replaceAll("\\", "/");
      const lines = readFileSync(file, "utf8").split("\n");
      lines.forEach((line, i) => {
        for (const [, arbitrary, utility] of line.matchAll(FOCUS_RING)) {
          // `ring-1`, `ring-inset`, `outline-offset-2` name no colour.
          const token = arbitrary ?? THEME_COLOURS.get(utility);
          if (!token || token === "--nimbus-focus") continue;

          let worst = Number.POSITIVE_INFINITY;
          let where = "";
          for (const [combo, t] of COMBOS) {
            for (const backdrop of BACKDROPS) {
              const ratio = contrast(t[token], t[backdrop]);
              if (ratio < worst) {
                worst = ratio;
                where = `${combo} on ${backdrop}`;
              }
            }
          }
          const why =
            worst < 3
              ? `${worst.toFixed(2)}:1 (${where}) is under the 3:1 floor`
              : `${worst.toFixed(2)}:1 (${where}) clears the floor, but a focus ring still names --nimbus-focus`;
          failures.push(`${name}:${i + 1} — ${token} — ${why}`);
        }
      });
    }
    // Named, not counted: a ratio without the file that paints it sends the
    // next person back to the browser to re-derive which ring is invisible.
    expect(failures).toEqual([]);
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
