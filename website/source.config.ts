import { defineConfig, defineDocs } from 'fumadocs-mdx/config';
import { pageSchema } from 'fumadocs-core/source/schema';
import { z } from 'zod';

// The content lives in `../docs`, the same Markdown the repository ships and
// the docs gates check. Nothing is copied into this package.
//
// `files` is the private fence. Only the six published groups compile, so
// `docs/private`, `docs/brand`, `docs/assets` and `docs/README.md` cannot
// reach the build even by accident.
const PUBLISHED_GROUPS = [
  'get-started',
  'developers',
  'agents',
  'operators',
  'concepts',
  'reference',
] as const;

export const docs = defineDocs({
  dir: '../docs',
  docs: {
    files: PUBLISHED_GROUPS.map((group) => `${group}/**/*.md`),
    // `sidebar` is Starlight frontmatter that the pages still carry. The
    // order drives the generated sidebar; the label overrides the title in
    // the sidebar only, so a long page title can still be short in the nav.
    schema: pageSchema.extend({
      description: z.string().min(1),
      sidebar: z
        .object({
          order: z.number().optional(),
          label: z.string().optional(),
        })
        .optional(),
    }),
    postprocess: { includeProcessedMarkdown: true },
  },
  // Every meta file is generated in `src/lib/sidebar.ts`. There are none on
  // disk, and `docs/` must not grow any.
  meta: { files: [] },
});

export default defineConfig();
