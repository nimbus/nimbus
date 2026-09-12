import { Mascot } from './mascot';

// The mark beside the wordmark. The wordmark is lowercase `nimbus` at
// semibold with -0.01em tracking, and the mark sits at 26px beside it.
export function Lockup() {
  return (
    <span className="flex items-center gap-2">
      <Mascot className="h-[22px] w-[26px]" />
      <span className="text-[15px] font-semibold tracking-[-0.01em] text-(--text-1)">
        nimbus
      </span>
    </span>
  );
}
