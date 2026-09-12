import type { Metadata } from 'next';
import Link from 'next/link';

import { Mascot } from '@/components/mascot';

export const metadata: Metadata = {
  alternates: { canonical: '/' },
};

// Placeholder. The Odyssey homepage lands here, rethemed on the Nimbus
// tokens.
export default function Page() {
  return (
    <main className="flex flex-1 flex-col items-center justify-center gap-6 px-6 text-center">
      <Mascot className="h-24 w-32" title="Nimbus" />
      <h1 className="text-[32px] leading-[38px] font-semibold tracking-[-0.02em] text-(--text-1)">
        Your cloud backend in one binary
      </h1>
      <p className="max-w-xl text-[16px] leading-6 text-(--text-2)">
        The single-binary backend for apps and AI agents. Drop-in compatible
        with Convex, Firestore, MongoDB, and DynamoDB.
      </p>
      <Link
        href="/docs/"
        className="rounded-md bg-(--accent) px-4 py-2 text-[14px] font-medium text-(--accent-ink) transition-colors hover:bg-(--accent-hover)"
      >
        Read the documentation
      </Link>
    </main>
  );
}
