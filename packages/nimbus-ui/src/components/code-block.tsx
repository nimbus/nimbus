import { useEffect, useRef, useState } from "react";
import {
  createHighlighterCore,
  type HighlighterCore,
  type ShikiTransformer,
} from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";

/** A per-identifier type hint (FSV8): 1-based line/col + the TS-compiler hover. */
export type CodeHint = {
  name: string;
  line: number;
  col: number;
  hover: string;
};

// A shiki transformer that, for each hint position, sets the matching token
// span's `title` to the inferred type (native hover tooltip) and marks it so
// CSS can show it's hoverable. shiki `span(node, line, col)` is 1-based line,
// 0-based col; the hints are 1-based line/col, so col maps to `col - 1`.
function typeHoverTransformer(hints: CodeHint[]): ShikiTransformer {
  const byPosition = new Map<string, string>();
  for (const hint of hints) {
    byPosition.set(`${hint.line}:${hint.col - 1}`, hint.hover);
  }
  return {
    name: "nimbus-type-hover",
    span(node, line, col) {
      const hover = byPosition.get(`${line}:${col}`);
      if (hover) {
        node.properties = node.properties ?? {};
        node.properties.title = hover;
        node.properties["data-typed"] = "true";
      }
    },
  };
}

// A shiki transformer that marks a single 1-based line so CSS can highlight it
// (and the scroll effect can find it). Used to land a reader on a run's error
// source line. shiki `line(node, line)` is 1-based.
function lineHighlightTransformer(highlightLine: number): ShikiTransformer {
  return {
    name: "nimbus-line-highlight",
    line(node, line) {
      if (line === highlightLine) {
        node.properties = node.properties ?? {};
        node.properties["data-highlighted-line"] = "true";
      }
    },
  };
}

const LIGHT_THEME = "github-light";
const DARK_THEME = "github-dark";
const LANGS = [
  "typescript",
  "tsx",
  "javascript",
  "jsx",
  "json",
  "bash",
] as const;

let highlighterPromise: Promise<HighlighterCore> | null = null;

function getHighlighter(): Promise<HighlighterCore> {
  if (!highlighterPromise) {
    highlighterPromise = createHighlighterCore({
      themes: [
        import("@shikijs/themes/github-light"),
        import("@shikijs/themes/github-dark"),
      ],
      langs: [
        import("@shikijs/langs/typescript"),
        import("@shikijs/langs/tsx"),
        import("@shikijs/langs/javascript"),
        import("@shikijs/langs/jsx"),
        import("@shikijs/langs/json"),
        import("@shikijs/langs/bash"),
      ],
      // JS regex engine (no WASM) so highlighting works under the embedded
      // console's `script-src 'self'` CSP, which forbids wasm-unsafe-eval.
      engine: createJavaScriptRegexEngine(),
    });
  }
  return highlighterPromise;
}

function normalizeLang(lang: string): string {
  const l = lang.toLowerCase();
  if (l === "ts") return "typescript";
  if (l === "js") return "javascript";
  return (LANGS as readonly string[]).includes(l) ? l : "typescript";
}

/**
 * Syntax-highlighted, theme-aware code block. Highlighting is async (shiki),
 * so it renders a plain monospace fallback until ready. Dual-theme output is
 * driven by CSS vars; see the `.nimbus-code` rules in globals.css.
 */
export function CodeBlock({
  code,
  lang = "typescript",
  hints,
  highlightLine,
  testid,
}: {
  code: string;
  lang?: string;
  hints?: CodeHint[];
  /** 1-based line to highlight and scroll into view (e.g. a run's error line). */
  highlightLine?: number;
  testid?: string;
}) {
  const [html, setHtml] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    const transformers: ShikiTransformer[] = [];
    if (hints && hints.length > 0)
      transformers.push(typeHoverTransformer(hints));
    if (highlightLine)
      transformers.push(lineHighlightTransformer(highlightLine));
    getHighlighter()
      .then((hl) =>
        hl.codeToHtml(code, {
          lang: normalizeLang(lang),
          themes: { light: LIGHT_THEME, dark: DARK_THEME },
          defaultColor: false,
          transformers,
        }),
      )
      .then((out) => {
        if (!cancelled) setHtml(out);
      })
      .catch(() => {
        if (!cancelled) setHtml(null);
      });
    return () => {
      cancelled = true;
    };
  }, [code, lang, hints, highlightLine]);

  // Once highlighted HTML is in the DOM, scroll the highlighted line into view.
  useEffect(() => {
    if (html === null || !highlightLine || !containerRef.current) return;
    const target = containerRef.current.querySelector(
      "[data-highlighted-line]",
    );
    target?.scrollIntoView({ block: "center" });
  }, [html, highlightLine]);

  if (html === null) {
    return (
      <pre
        className="m-0 h-full overflow-auto whitespace-pre bg-surface-2 p-3 font-mono text-sm leading-[1.5] text-default"
        data-testid={testid}
      >
        {code}
      </pre>
    );
  }

  return (
    <div
      ref={containerRef}
      // 1.5 leading, per DESIGN.md's code-block spec; `leading-5` was 1.667.
      className="nimbus-code h-full overflow-auto text-sm leading-[1.5]"
      data-testid={testid}
      // biome-ignore lint/security/noDangerouslySetInnerHtml: Shiki escapes source text before it returns this highlighted markup.
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
