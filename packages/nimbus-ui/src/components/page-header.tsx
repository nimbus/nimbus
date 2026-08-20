import type { ReactNode } from "react";

import { cn } from "../lib/cn";

/**
 * Canonical page header for every route surface: a title, an optional muted
 * subtitle, and an optional right-aligned trailing slot (count chip, summary,
 * scope chip). Replaces the per-page hand-rolled `<header>` molecule so the
 * title/subtitle/trailing layout stays identical across Developer and Operator
 * consoles.
 */
export function PageHeader({
  title,
  subtitle,
  trailing,
  testid,
  className,
}: {
  title: string;
  subtitle?: ReactNode;
  trailing?: ReactNode;
  testid?: string;
  className?: string;
}) {
  return (
    <header
      className={cn("flex items-baseline justify-between gap-4", className)}
      data-testid={testid}
    >
      <div className="min-w-0">
        <h1
          className="text-xl text-default"
          style={{ fontSize: "var(--text-xl)" }}
        >
          {title}
        </h1>
        {/* No measure cap. The paragraph is shrink-to-fit inside a header
            row, so its width is the sentence's own intrinsic width and does
            not grow with the viewport. Capping it only bought a second line
            on 12 of 23 routes at every desktop width, which moved the panel
            below by one line-height as you navigated. Subtitle length is
            bounded where the prose is authored instead; see
            `subtitle-measure.spec.ts`. The `max-w` that remains is a runaway
            guard for prose that spec cannot read statically -- at 110ch it
            never binds on copy inside that budget. */}
        {subtitle ? (
          <p
            data-slot="page-subtitle"
            className="max-w-[110ch] text-sm text-muted"
          >
            {subtitle}
          </p>
        ) : null}
      </div>
      {trailing ? <div className="shrink-0">{trailing}</div> : null}
    </header>
  );
}
