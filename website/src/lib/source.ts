import { docs } from 'collections/server';
import { loader } from 'fumadocs-core/source';

import { withSidebar } from './sidebar';

// `docs/get-started/quickstart.md` is served at `/get-started/quickstart/`,
// the same URL the Starlight site used. Every internal link in the content is
// absolute and slash-terminated, so the map has to stay exactly this.
//
// `page.url` itself carries no trailing slash: fumadocs normalizes it away,
// and Next adds it back under `trailingSlash: true` when it renders a link or
// a canonical tag. Passing a `url` option here would not change that. Raw
// text that Next never touches has to add the slash itself, which is what
// `pageUrl` is for.
export const source = loader({
  baseUrl: '/',
  source: withSidebar(docs.toFumadocsSource()),
});

/** The canonical, slash-terminated address of a URL inside the site. */
export function canonical(url: string): string {
  const mark = url.search(/[#?]/);
  const path = mark === -1 ? url : url.slice(0, mark);
  const suffix = mark === -1 ? '' : url.slice(mark);
  return path.endsWith('/') ? url : `${path}/${suffix}`;
}

/** The canonical address of a page. */
export function pageUrl(page: (typeof source)['$inferPage']): string {
  return canonical(page.url);
}
