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
        {/* Cap the measure: subtitles run 130-170 characters and would
            otherwise set a single ~145-character line at 1440px and keep
            growing. `EmptyState` already caps its body the same way. */}
        {subtitle ? (
          <p className="max-w-[68ch] text-sm text-muted">{subtitle}</p>
        ) : null}
      </div>
      {trailing ? <div className="shrink-0">{trailing}</div> : null}
    </header>
  );
}
