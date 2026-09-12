import { createFromSource } from 'fumadocs-core/search/server';

import { source } from '@/lib/source';

// The site is a static export, so the index is built once and downloaded by
// the browser. `staticGET` writes it; `staticClient` in the dialog reads it.
export const revalidate = false;

export const { staticGET: GET } = createFromSource(source, {
  language: 'english',
});
