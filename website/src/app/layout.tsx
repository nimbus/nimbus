import type { Metadata } from 'next';
import type { ReactNode } from 'react';
import { RootProvider } from 'fumadocs-ui/provider/next';

import './global.css';

export const metadata: Metadata = {
  metadataBase: new URL('https://nimbusdocs.com'),
  title: {
    default: 'Nimbus',
    template: '%s | Nimbus',
  },
  description:
    'The single-binary backend for apps and AI agents. Drop-in compatible with Convex, Firestore, MongoDB, and DynamoDB.',
  icons: {
    icon: [
      { url: '/favicon.ico', sizes: '16x16 32x32 48x48' },
      { url: '/favicon.svg', type: 'image/svg+xml', sizes: 'any' },
    ],
    apple: { url: '/icon-512.png', sizes: '512x512' },
  },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="flex min-h-screen flex-col">
        {/* Search is a static index built at export time. DR5 turns it on. */}
        <RootProvider
          search={{ enabled: false }}
          theme={{ storageKey: 'nimbus-docs-theme' }}
        >
          {children}
        </RootProvider>
      </body>
    </html>
  );
}
