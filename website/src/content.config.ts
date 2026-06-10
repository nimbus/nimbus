import { defineCollection } from 'astro:content';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

// DOC1 baseline: default loader over src/content/docs (placeholder landing only).
// DOC3 replaces this with explicit glob loaders over the five public groups:
// ../docs/{get-started,developers,operators,concepts,reference} — an allow-list,
// so nothing else under docs/ can publish by accident.
export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
};
