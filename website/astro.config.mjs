// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLlmsTxt from 'starlight-llms-txt';

// Renderer only: content is authored in ../docs (the five public groups).
// DOC1 builds against a placeholder landing; DOC3 wires the content-layer
// glob loaders at ../docs/{get-started,developers,operators,concepts,reference}.
// Theme tokens map DESIGN.md's product palette onto Starlight — see
// src/styles/custom.css and DESIGN.md §Documentation site.
export default defineConfig({
  site: 'https://nimbusdocs.com',
  integrations: [
    starlight({
      title: 'Nimbus',
      description:
        'The single-binary backend for apps and AI agents. Drop-in compatible with Convex, Firestore, MongoDB, and DynamoDB.',
      favicon: '/favicon.svg',
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/nimbus/nimbus',
        },
      ],
      customCss: [
        '@fontsource-variable/jetbrains-mono',
        './src/styles/custom.css',
      ],
      sidebar: [
        { label: 'Get started', items: [{ autogenerate: { directory: 'get-started' } }] },
        { label: 'Developers', items: [{ autogenerate: { directory: 'developers' } }] },
        { label: 'Operators', items: [{ autogenerate: { directory: 'operators' } }] },
        { label: 'Concepts', items: [{ autogenerate: { directory: 'concepts' } }] },
        { label: 'Reference', items: [{ autogenerate: { directory: 'reference' } }] },
      ],
      plugins: [
        starlightLlmsTxt({
          projectName: 'Nimbus',
        }),
      ],
    }),
  ],
});
