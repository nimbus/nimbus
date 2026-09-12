import { createMDX } from 'fumadocs-mdx/next';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const config = {
  // Cloudflare serves `out/` as static assets. There is no server.
  output: 'export',
  // Every internal link in `docs/` is absolute and ends in a slash. Trailing
  // slashes keep those links correct and keep the Starlight URLs unchanged.
  trailingSlash: true,
  reactStrictMode: true,
  poweredByHeader: false,
  // The content is `../docs`, above this package. The workspace root has to
  // cover it or the generated imports resolve outside the compilation scope.
  turbopack: { root: resolve(here, '..') },
};

export default withMDX(config);
