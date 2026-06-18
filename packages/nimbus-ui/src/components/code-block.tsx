import { useEffect, useState } from "react";
import { createHighlighter, type Highlighter } from "shiki";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";

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

let highlighterPromise: Promise<Highlighter> | null = null;

function getHighlighter(): Promise<Highlighter> {
  if (!highlighterPromise) {
    highlighterPromise = createHighlighter({
      themes: [LIGHT_THEME, DARK_THEME],
      langs: [...LANGS],
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
  testid,
}: {
  code: string;
  lang?: string;
  testid?: string;
}) {
  const [html, setHtml] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getHighlighter()
      .then((hl) =>
        hl.codeToHtml(code, {
          lang: normalizeLang(lang),
          themes: { light: LIGHT_THEME, dark: DARK_THEME },
          defaultColor: false,
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
  }, [code, lang]);

  if (html === null) {
    return (
      <pre
        className="m-0 h-full overflow-auto whitespace-pre bg-surface p-4 font-mono text-[12px] leading-5 text-default"
        data-testid={testid}
      >
        {code}
      </pre>
    );
  }

  return (
    // biome-ignore lint/security/noDangerouslySetInnerHtml: trusted shiki highlighter output built from source text
    <div
      className="nimbus-code h-full overflow-auto text-[12px] leading-5"
      data-testid={testid}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
