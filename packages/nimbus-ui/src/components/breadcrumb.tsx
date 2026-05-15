import { Link } from "@tanstack/react-router";

import { CopyChip } from "./copy-chip";

export type BreadcrumbSegment = {
  label: string;
  href?: string;
  copyValue?: string;
  copyLabel?: string;
  active?: boolean;
};

export function Breadcrumb({
  segments,
  testid,
}: {
  segments: BreadcrumbSegment[];
  testid?: string;
}) {
  return (
    <nav
      aria-label="Resource breadcrumb"
      className="flex items-center gap-1 font-mono text-xs text-muted"
      data-testid={testid ?? "resource-breadcrumb"}
    >
      {segments.map((segment, idx) => (
        <span
          // biome-ignore lint/suspicious/noArrayIndexKey: breadcrumb segments are positional by design and cannot reorder
          key={`${segment.label}-${idx}`}
          className="group inline-flex items-center gap-1"
        >
          {idx > 0 ? (
            <span aria-hidden="true" className="text-muted">
              ›
            </span>
          ) : null}
          {segment.href && !segment.active ? (
            <Link
              to={segment.href}
              className="text-muted hover:text-default focus-visible:text-default"
              data-testid={`breadcrumb-link-${idx}`}
            >
              {segment.label}
            </Link>
          ) : (
            <span
              className={segment.active ? "text-default" : "text-muted"}
              data-testid={`breadcrumb-segment-${idx}`}
            >
              {segment.label}
            </span>
          )}
          {segment.copyValue ? (
            <CopyChip
              label={segment.copyLabel ?? segment.label}
              value={segment.copyValue}
              hideUntilHover
              testid={`breadcrumb-copy-${idx}`}
              className="text-[10px]"
            >
              copy
            </CopyChip>
          ) : null}
        </span>
      ))}
    </nav>
  );
}
