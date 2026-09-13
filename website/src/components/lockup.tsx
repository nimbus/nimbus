import { Mascot } from './mascot';

// The mark beside the wordmark. The wordmark is lowercase `nimbus` at
// semibold with -0.01em tracking, and the mark sits at 32px beside it. On
// desktop the sidebar row grows to whatever the mark asks for; below the md
// breakpoint the header band is a fixed 56px, so 32 is the largest height
// that still leaves the mark air on both sides. Same height as the Odyssey
// brand row and the console sidebar, so the three read as one mark.
export function Lockup() {
  return (
    <span className="flex items-center gap-2">
      <Mascot className="h-[32px] w-auto" />
      <span className="text-[15px] font-semibold tracking-[-0.01em] text-(--text-1)">
        nimbus
      </span>
    </span>
  );
}
