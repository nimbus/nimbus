import { Link } from "@tanstack/react-router";
import { Fragment } from "react";

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
        // The separator is a sibling of the segment, not a child of it, for
        // two reasons: the nav's own `gap-1` then spaces it identically from
        // both neighbours, and the hover `group` closes tightly around the
        // label so pointing at a chevron no longer reveals the next segment's
        // copy chip.
        // biome-ignore lint/suspicious/noArrayIndexKey: breadcrumb segments are positional by design and cannot reorder
        <Fragment key={`${segment.label}-${idx}`}>
          {idx > 0 ? (
            <span aria-hidden="true" className="text-muted">
              ›
            </span>
          ) : null}
          <span className="group relative inline-flex items-center">
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
            {/* Out of flow, below the label. A chip that reveals itself in
                flow moves everything after it — CopyChip collapses to `w-0`
                at rest, so revealing it pushed the next chevron sideways by
                the chip's full width, and even collapsed it still drew the
                row's `gap`. Absolute positioning costs zero width at rest and
                zero width when shown, so the trail never reflows. */}
            {segment.copyValue ? (
              <CopyChip
                label={segment.copyLabel ?? segment.label}
                value={segment.copyValue}
                hideUntilHover
                testid={`breadcrumb-copy-${idx}`}
                className="absolute left-0 top-full z-10 mt-1 rounded border border-app bg-surface-2 text-xs shadow-sm"
              >
                copy
              </CopyChip>
            ) : null}
          </span>
        </Fragment>
      ))}
    </nav>
  );
}
