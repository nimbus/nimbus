// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLlmsTxt from 'starlight-llms-txt';

// Renderer only: content is authored in ../docs (the five public groups).
// DOC1 builds against a placeholder landing; DOC3 wires the content-layer
// glob loaders at ../docs/{get-started,developers,operators,concepts,reference}.
export default defineConfig({
  site: 'https://nimbusdocs.com',
  integrations: [
    starlight({
      title: 'Nimbus',
      description:
        'The single-binary backend for apps and AI agents. Drop-in compatible with Convex, Firestore, MongoDB, and DynamoDB.',
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/nimbus/nimbus',
        },
      ],
      plugins: [
        starlightLlmsTxt({
          projectName: 'Nimbus',
        }),
      ],
    }),
  ],
});
