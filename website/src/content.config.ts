import { defineCollection } from 'astro:content';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

// Allow-list by construction: src/content/docs/ contains ONLY the splash
// landing (index.mdx) plus five symlinks — get-started, developers,
// operators, concepts, reference — each resolving into ../docs at the repo
// root. docsLoader() follows the symlinks, so exactly those five public
// groups can publish; nothing else under docs/ (notably docs/private/) is
// reachable. Verified by scripts/verify-nimbus-docs-site.sh condition 3.
export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
};
