import type { SVGProps } from 'react';

// The Nimbus mark: the same drawing as `packages/nimbus-ui/src/components/
// mascot.tsx`, in its resting state. The console owns the animated states; the
// docs site only ever shows the mark.
//
// The body takes `--mark` and the face takes `--mark-ink`. Both hold the gold
// on every ground, so this is the same sticker as the favicon and the app icon.
export function Mascot({
  title,
  ...props
}: SVGProps<SVGSVGElement> & { title?: string }) {
  return (
    <svg
      viewBox="12 8 96 80"
      xmlns="http://www.w3.org/2000/svg"
      role={title ? 'img' : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      {...props}
    >
      <g fill="var(--mark)">
        <circle cx="36" cy="50" r="20" />
        <circle cx="60" cy="40" r="28" />
        <circle cx="84" cy="50" r="20" />
        <rect x="14" y="50" width="92" height="34" rx="17" />
      </g>
      <g fill="var(--mark-ink)">
        <circle cx="48" cy="52" r="3.8" />
        <circle cx="72" cy="52" r="3.8" />
      </g>
      <path
        d="M52 62 q8 8 16 0"
        fill="none"
        stroke="var(--mark-ink)"
        strokeWidth="4"
        strokeLinecap="round"
      />
    </svg>
  );
}
