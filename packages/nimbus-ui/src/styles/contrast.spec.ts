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
  "--accent-text",
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

  /* --accent-text is the one accent role measured as a foreground, and the
     rule that keeps it from becoming a third gold is that it is never a
     colour of its own: it is whichever member of the accent pair the ground
     can show. Without this the token is just `--accent-edge` again, and the
     darkened gold that used to live in that slot (#866423) would pass the
     4.5:1 check above while reading as a different colour standing next to
     the mascot. TEXT_TOKENS holds the floor; this holds the palette. */
  it.each(COMBOS)("%s: --accent-text is the accent or its ink", (_, t) => {
    expect([t["--accent"], t["--accent-ink"]]).toContain(t["--accent-text"]);
  });

  /* Which one it is follows from the ground rather than from taste: on a
     ground that can carry the gold as text the token must be the gold, and
     only where the gold fails may it fall back to the ink. Asserting the
     direction stops a theme from quietly dropping to the ink everywhere and
     passing the two checks above with no accent left in it. */
  it.each(
    COMBOS,
  )("%s: it keeps the gold wherever the gold can be read", (_, t) => {
    const goldIsReadable = GROUNDS.every(
      (ground) => contrast(t["--accent"], t[ground]) >= 4.5,
    );
    expect(t["--accent-text"]).toBe(
      goldIsReadable ? t["--accent"] : t["--accent-ink"],
    );
  });

  // --accent is the gold, and the gold is deliberately NOT held to a floor
  // against the grounds: at 1.60:1 on light --bg-hover it would fail, and
  // darkening it to pass is what turned the light accent into a different
  // colour from the mark. What is held is the pair that makes it legible --
  // the ink, which serves both as the ink on top of a gold fill and as the
  // carrier or keyline under a gold mark that has no room for ink.
  it.each(COMBOS)("%s: every ink clears AA on its own fill", (_, t) => {
    for (const [ink, fill] of [
      ["--accent-ink", "--accent"],
      ["--accent-ink", "--accent-hover"],
      ["--error-ink", "--error"],
      ["--success-ink", "--success"],
    ]) {
      expect(contrast(t[ink], t[fill])).toBeGreaterThanOrEqual(4.5);
    }
  });

  // The gold is the same literal in both themes. The accent is the identity,
  // the mark is the identity, and light must not fork them again.
  it("uses one gold for --accent and --mark in both themes", () => {
    for (const t of Object.values(THEMES)) {
      expect(t["--accent"]).toBe(t["--mark"]);
    }
    expect(THEMES.light["--accent"]).toBe(THEMES.dark["--accent"]);
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
   paint the gold with its keyline (DESIGN.md accent job 3). And no Nimbus-owned
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
   with the ring (DESIGN.md inputs). That one is admitted because the global
   ring is drawn immediately outside it and carries the keyline, so the gold
   border never has to answer the white canvas by itself. Any other focus
   border is a private indicator on a token measured for something else. */
const RING_COLOUR =
  /\b(?:focus|focus-visible|focus-within):(?:ring|outline)-(?:\[color:var\((--[a-z0-9-]+)\)\]|(?:accent|accent-text|accent-ink|success|warning|error|info|text-[1-4]|border-[1-3]|ring|destructive|bg-[a-z]+))(?:\/\d+)?(?![\w-])/g;
const BORDER_COLOUR =
  /\b(?:focus|focus-visible|focus-within):border-(?:\[color:var\((--[a-z0-9-]+)\)\]|(?:accent-(?:text|ink|hover)|success|warning|error|info|text-[1-4]|border-[1-3]|ring|destructive|bg-[a-z]+))(?:\/\d+)?(?![\w-])/g;

/** Every .tsx under a directory, the vendored registry included. */
function allTsxFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...allTsxFiles(path));
    else if (entry.name.endsWith(".tsx") && !entry.name.includes(".spec."))
      out.push(path);
  }
  return out;
}

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
  it("paints the global ring as the gold plus its keyline, unlayered", () => {
    const rule = unlayeredRules(GLOBALS).find((r) =>
      r.startsWith(":focus-visible {"),
    );
    expect(rule).toBeDefined();
    expect(rule).toMatch(/outline:\s*none;/);
    // The gold inside, the keyline one pixel wider, so the keyline shows as
    // a hairline around the gold rather than replacing it.
    expect(rule).toMatch(/0 0 0 2px var\(--accent\)/);
    expect(rule).toMatch(/0 0 0 3px var\(--accent-ink\)/);
  });

  /* Both strokes have to be opaque. The ring this replaced was
     `color-mix(in srgb, var(--accent-edge) 40%, transparent)`: the token it
     named measured 4.61:1 against the light canvas and the stroke that
     actually reached the screen measured 1.77:1, because 40% of it was the
     canvas. Every token-level check in this file passed throughout. Nothing
     but this test can tell the difference, so it is the only thing standing
     between the ring and that defect returning. */
  it("composites no part of the ring against the ground", () => {
    const rule = unlayeredRules(GLOBALS).find((r) =>
      r.startsWith(":focus-visible {"),
    );
    const shadow = rule?.match(/box-shadow:([\s\S]*?);/)?.[1] ?? "";
    expect(shadow).not.toMatch(/transparent|color-mix|\brgba?\(|\/\s*\d/);
  });

  /* And the pair has to work as an indicator in both themes. SC 2.4.13 reads
     a focus indicator against the colour next to it, so what matters is that
     the two strokes are distinct from each other and that whichever one meets
     the page is distinct from the page. The gold answers the dark grounds and
     the keyline answers the light ones, which is the whole reason the ring is
     two strokes and not one. */
  it.each(COMBOS)("%s: the ring reads against every ground", (_, t) => {
    expect(contrast(t["--accent"], t["--accent-ink"])).toBeGreaterThanOrEqual(
      3,
    );
    for (const ground of GROUNDS) {
      const best = Math.max(
        contrast(t["--accent"], t[ground]),
        contrast(t["--accent-ink"], t[ground]),
      );
      expect(best).toBeGreaterThanOrEqual(3);
    }
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
    expect(rule?.[1]).toMatch(/color:\s*var\(--accent-text\)/);
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

// --- the gold names its carrier -------------------------------------------

/* `--accent` is Nimbus gold in both themes, which means it is 1.60:1 on the
   darkest light ground and 10.45:1 on the darkest dark one. It is never
   legible on its own across both themes, in any shape -- as a fill, as a
   hairline, or as text. What makes it legible is always the same thing: the
   `--accent-ink` next to it, as ink on top of a fill, as a carrier behind
   text, or as a keyline around a mark too thin to hold ink.

   So the rule is not which utility may paint the gold -- it is that whatever
   does must name the carrier in the same breath. A class string that paints
   the gold and says nothing about `--accent-ink` is a gold mark with no
   ground, which is the defect this whole section exists to catch, and the
   one the retired `--accent-edge` used to paper over by darkening the hue.

   Matching runs over one string literal at a time rather than the file,
   because a className is a literal and the carrier has to be on the same
   element, not merely somewhere in the module. */
const GOLD =
  /\b(?:bg|text|border|divide|outline|ring|decoration|fill|stroke|shadow)-accent(?:-hover)?(?![\w-])/;
const CARRIER =
  /\b(?:bg|text|border|divide|outline|ring|decoration|fill|stroke|shadow)-accent-ink(?![\w-])|var\(--accent-ink\)/;

/* The exception, and the only one: the gold under a focus variant. The global
   `:focus-visible` rule draws the keyline immediately outside whatever the
   variant paints, so that gold is carried by a rule in globals.css rather
   than by a class beside it. `focus indicators` above is what holds the
   keyline in place, and BORDER_COLOUR is what keeps the exception down to the
   one border it is written for. */
const FOCUS_GOLD =
  /\b(?:focus|focus-visible|focus-within):(?:bg|text|border|ring|outline)-accent(?:-hover)?(?![\w-])/g;
const STRING_LITERAL =
  /"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`/g;

describe("the gold names its carrier", () => {
  /* This runs over the registry as well. `components/ui` is exempt from the
     Nimbus-owned rules elsewhere in this file because it is vendored and
     speaks shadcn's vocabulary, but "the gold needs its ink" is a contrast
     fact and holds no matter who wrote the file. The registry is in fact
     where it was broken: dropdown-menu painted the gold under `--text-1`,
     1.76:1. */
  it("gives every gold mark a carrier, registry included", () => {
    const offenders: string[] = [];
    for (const file of allTsxFiles(SRC)) {
      const text = readFileSync(file, "utf8");
      for (const literal of text.matchAll(STRING_LITERAL)) {
        const classes = literal[0].replace(FOCUS_GOLD, "");
        if (!GOLD.test(classes)) continue;
        if (CARRIER.test(classes)) continue;
        const line = text.slice(0, literal.index).split("\n").length;
        offenders.push(
          `${relative(SRC, file)}:${line} ${GOLD.exec(classes)?.[0]} with no carrier`,
        );
      }
    }
    expect(offenders).toEqual([]);
  });

  /* The shadcn bridge resolves the registry's vocabulary onto the role
     tokens. `ring`/`sidebar-ring` are the registry's focus indicator, and
     the global `:focus-visible` rule overpaints both with the gold and its
     keyline -- so what these tokens have to be is the gold, the colour that
     shows through if that rule is ever removed. Naming a hue nothing else in
     the sheet uses is how the palette grew a third gold the last time. */
  it("routes the registry ring to the gold", () => {
    // The bridge is the `@theme inline` block, not `:root`: these are
    // Tailwind utility-namespace names, not role tokens.
    const bridge = block("@theme inline");
    for (const token of ["--color-ring", "--color-sidebar-ring"]) {
      expect(bridge[token]).toBe("var(--accent)");
    }
  });

  /* Series 1 is the gold, like every other accent mark. A series is the one
     accent shape that carries no ink of its own and cannot be given a
     carrier by a class beside it, so the first chart built here owes series 1
     an `--accent-ink` hairline around the fill. Nothing charts anything yet;
     what this holds is that the answer when something does is the gold and
     not a second hue invented for the occasion. */
  it("routes the first chart series to the gold", () => {
    expect(block("@theme inline")["--color-chart-1"]).toBe("var(--accent)");
  });
});

// --- every ink is legible on its own ground -------------------------------

/* The bridge names grounds and inks in pairs: `popover`/`popover-foreground`,
   `primary`/`primary-foreground`. The pair is a promise that the ink can be
   read on that ground, and nothing but this test keeps the promise -- a name
   that points at a token which merely sounds related still compiles and still
   renders. Both real failures looked exactly like that:
   `accent-foreground` pointed at `--text-1` (1.76:1 on the gold, because the
   gold is a light fill wanting dark ink) and `destructive-foreground` pointed
   at `--error` itself (1.00:1, ink on its own colour).

   Deriving the pairs from the sheet rather than listing them means a pair
   added later is held to the floor without anyone remembering to add it. */
function resolve(value: string, theme: Tokens): string {
  const seen = new Set<string>();
  let v = value.trim();
  while (v.startsWith("var(")) {
    const name = v.slice(4, v.indexOf(")"));
    if (seen.has(name)) throw new Error(`cyclic token ${name}`);
    seen.add(name);
    const next = theme[name] ?? BRIDGE[name];
    if (next === undefined) throw new Error(`unresolved token ${name}`);
    v = next.trim();
  }
  return v;
}

const BRIDGE = block("@theme inline");

/** Ground for each `<name>-foreground`. shadcn pairs the page ink with
 *  `background`, which is the one pair whose ground is not its own prefix. */
function groundFor(ink: string): string {
  return ink === "--color-foreground"
    ? "--color-background"
    : ink.slice(0, -"-foreground".length);
}

const INK_PAIRS = Object.keys(BRIDGE)
  .filter((name) => name.endsWith("-foreground"))
  .map((ink) => [groundFor(ink), ink] as const);

describe("every ink is legible on its own ground", () => {
  it("pairs a ground with every ink the bridge names", () => {
    // Guards the derivation itself: a rename that breaks the `-foreground`
    // convention would otherwise empty this suite and look green.
    expect(INK_PAIRS.length).toBeGreaterThanOrEqual(10);
    for (const [ground] of INK_PAIRS) expect(BRIDGE[ground]).toBeDefined();
  });

  it.each(COMBOS)("%s: every bridge pair clears 4.5:1", (_, theme) => {
    const failures: string[] = [];
    for (const [ground, ink] of INK_PAIRS) {
      const ratio = contrast(
        resolve(BRIDGE[ink], theme),
        resolve(BRIDGE[ground], theme),
      );
      if (ratio < 4.5) {
        failures.push(`${ink} on ${ground} is ${ratio.toFixed(2)}:1`);
      }
    }
    expect(failures).toEqual([]);
  });
});
