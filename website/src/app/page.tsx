import type { Metadata } from 'next';

import { Journey } from '@/components/odyssey/journey';
import { OG_IMAGE } from '@/lib/og';

export const metadata: Metadata = {
  // Starlight composed the tab title as "page | site", which would have read
  // "nimbus | nimbus" here. The home page names itself instead.
  title: 'nimbus — your cloud, one binary',
  description:
    'The single-binary backend for apps and AI agents. Drop-in compatible with Convex, Firestore, MongoDB, and DynamoDB.',
  alternates: { canonical: '/' },
  openGraph: {
    title: 'nimbus — your cloud, one binary',
    description:
      'The single-binary backend for apps and AI agents. Drop-in compatible with Convex, Firestore, MongoDB, and DynamoDB.',
    url: '/',
    siteName: 'Nimbus',
    type: 'website',
    images: [OG_IMAGE],
  },
};

// The home page is the journey: one scroll-driven scene that follows a
// request from an app through the binary and back. It is a client component
// because the whole page is a canvas the scroll position drives; this file
// stays a server component so the route keeps its metadata.
export default function Page() {
  return <Journey />;
}
