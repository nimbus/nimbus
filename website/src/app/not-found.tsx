import Link from 'next/link';

import { Mascot } from '@/components/mascot';

export default function NotFound() {
  return (
    <main className="flex flex-1 flex-col items-center justify-center gap-4 px-6 text-center">
      <Mascot className="h-16 w-[85px]" />
      <h1 className="text-[24px] leading-8 font-semibold tracking-[-0.01em] text-(--text-1)">
        This page is not here
      </h1>
      <p className="text-[14px] text-(--text-2)">
        The address is wrong, or the page moved.
      </p>
      <Link href="/docs/" className="text-[14px] text-(--accent-text) underline">
        Go to the documentation
      </Link>
    </main>
  );
}
