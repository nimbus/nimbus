import { ServerCodeBlock } from 'fumadocs-ui/components/codeblock.rsc';

// The MDX pipeline highlights every fenced block in `docs/` with the fumadocs
// defaults: two Shiki themes emitted side by side as `--shiki-light` and
// `--shiki-dark` custom properties, which `docs-theme.css` then selects
// between. A hand-authored page has no MDX pipeline, so it has to ask for the
// same pair explicitly or its code would arrive unstyled in one scheme.
const THEMES = { light: 'github-light', dark: 'github-dark' } as const;

/**
 * A fenced code block for a page written in TSX rather than MDX, rendered
 * with the same highlighter and themes as the documentation body.
 */
export function Code({
  lang,
  code,
  title,
}: {
  lang: string;
  code: string;
  title?: string;
}) {
  return (
    <ServerCodeBlock
      code={code}
      lang={lang}
      themes={THEMES}
      defaultColor={false}
      codeblock={{ title }}
    />
  );
}
