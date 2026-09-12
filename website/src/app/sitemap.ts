import type { MetadataRoute } from 'next';

import { pageUrl, source } from '@/lib/source';

const SITE = 'https://nimbusdocs.com';

export const revalidate = false;

// The home page, the documentation landing, and every published page. The
// export writes this to `sitemap.xml`.
export default function sitemap(): MetadataRoute.Sitemap {
  return [
    { url: `${SITE}/`, priority: 1 },
    { url: `${SITE}/docs/`, priority: 0.8 },
    ...source.getPages().map((page) => ({
      url: `${SITE}${pageUrl(page)}`,
      priority: 0.5,
    })),
  ];
}
