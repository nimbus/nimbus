import { docs } from 'collections/server';
import { loader } from 'fumadocs-core/source';

import { withSidebar } from './sidebar';

// `docs/get-started/quickstart.md` is served at `/get-started/quickstart/`.
// The Starlight site used the same URLs and every internal link in the content
// is absolute and slash-terminated, so the map has to stay exactly this.
export const source = loader({
  baseUrl: '/',
  url: (slugs) => `/${slugs.join('/')}/`,
  source: withSidebar(docs.toFumadocsSource()),
});
