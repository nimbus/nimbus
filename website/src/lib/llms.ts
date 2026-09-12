import { llms } from 'fumadocs-core/source';

import { canonical, pageUrl, source } from './source';

type DocsPage = (typeof source)['$inferPage'];

// `llms-small.txt` is the reading list for an agent with a small budget. It
// drops the two bodies of text that are deep rather than broad: the
// architecture series, which explains how the engine is built rather than how
// to use it, and the per-protocol reference directories, which are method
// tables best read one page at a time.
const SMALL_EXCLUDES = [
  'concepts/architecture',
  'reference/cloud-functions',
  'reference/cloudflare',
  'reference/convex',
  'reference/dynamodb',
  'reference/firebase',
  'reference/kv',
  'reference/mongodb',
  'reference/native',
  'reference/runtimes',
  'reference/s3',
];

function inSmall(page: DocsPage): boolean {
  const path = page.slugs.join('/');
  return !SMALL_EXCLUDES.some(
    (prefix) => path === prefix || path.startsWith(`${prefix}/`),
  );
}

/**
 * One page as Markdown, titled and addressed, so a reader that lands in the
 * middle of a concatenated file still knows which page it is reading.
 */
export async function renderPage(page: DocsPage): Promise<string> {
  const body = await page.data.getText('processed');
  return `# ${page.data.title} (${pageUrl(page)})\n\n${body}`;
}

// The index is built from the page tree, which holds unslashed URLs. Rewrite
// the link targets so a reader that follows one does not pay a redirect.
function slashLinks(markdown: string): string {
  return markdown.replace(
    /\]\((\/[^)\s]*)\)/g,
    (_match, url: string) => `](${canonical(url)})`,
  );
}

const catalogue = llms(source, { renderPage });

/** `llms.txt`: the map. Every page as a titled link under its section. */
export async function llmsIndex(): Promise<string> {
  return slashLinks(await catalogue.index());
}

/** `llms-full.txt`: every published page, in full. */
export function llmsFull(): Promise<string> {
  return catalogue.full();
}

/** `llms-small.txt`: the same, without the deep reference material. */
export async function llmsSmall(): Promise<string> {
  const pages = source.getPages().filter(inSmall);
  const rendered = await Promise.all(pages.map(renderPage));
  return rendered.join('\n\n');
}
