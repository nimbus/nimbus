import type { ReactNode } from "react";

import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

export { Skeleton };

// LoadingStatus wraps a shaped skeleton in the one polite status a screen
// reader hears, so a page never announces every bar it draws.
export function LoadingStatus({
  label = "Loading",
  testid,
  className,
  children,
}: {
  label?: string;
  testid?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      role="status"
      aria-live="polite"
      aria-busy="true"
      data-testid={testid}
      className={className}
    >
      <span className="sr-only">{label}</span>
      {children}
    </div>
  );
}

const WIDTHS = ["w-2/5", "w-1/4", "w-1/5", "w-1/6", "w-1/3", "w-1/4"] as const;

// TableSkeleton stands in for a dense table: a header row and `rows` body
// rows inside the panel frame the table takes when it arrives.
export function TableSkeleton({
  rows = 6,
  columns = 4,
  label,
  testid,
  className,
}: {
  rows?: number;
  columns?: number;
  label?: string;
  testid?: string;
  className?: string;
}) {
  const grid = { gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` };
  return (
    <LoadingStatus
      label={label}
      testid={testid}
      className={cn(
        "overflow-hidden rounded-md border border-border-1 bg-bg-panel",
        className,
      )}
    >
      <div
        className="grid gap-6 border-b border-border-1 px-4 py-3"
        style={grid}
      >
        {Array.from({ length: columns }, (_, column) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: placeholder cells are positional and never reorder
          <Skeleton key={column} className="h-3 w-16" />
        ))}
      </div>
      {Array.from({ length: rows }, (_, row) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: placeholder rows are positional and never reorder
          key={row}
          data-testid="skeleton-row"
          className="grid gap-6 border-b border-border-1 px-4 py-3.5 last:border-b-0"
          style={grid}
        >
          {Array.from({ length: columns }, (_, column) => (
            <Skeleton
              // biome-ignore lint/suspicious/noArrayIndexKey: placeholder cells are positional and never reorder
              key={column}
              className={cn("h-3.5", WIDTHS[(row + column) % WIDTHS.length])}
            />
          ))}
        </div>
      ))}
    </LoadingStatus>
  );
}

// CardSkeleton stands in for a card with a title and `lines` rows of text.
export function CardSkeleton({
  lines = 3,
  label,
  className,
}: {
  lines?: number;
  label?: string;
  className?: string;
}) {
  return (
    <LoadingStatus
      label={label}
      className={cn(
        "rounded-md border border-border-1 bg-bg-panel p-5",
        className,
      )}
    >
      <Skeleton className="mb-4 h-3.5 w-28" />
      <div className="flex flex-col gap-3">
        {Array.from({ length: lines }, (_, line) => (
          <Skeleton
            // biome-ignore lint/suspicious/noArrayIndexKey: placeholder lines are positional and never reorder
            key={line}
            className={cn("h-3.5", WIDTHS[line % WIDTHS.length])}
          />
        ))}
      </div>
    </LoadingStatus>
  );
}

// CardGridSkeleton stands in for a grid of entity cards: an icon, a name,
// and two lines per card.
export function CardGridSkeleton({
  count = 4,
  label,
  className,
}: {
  count?: number;
  label?: string;
  className?: string;
}) {
  return (
    <LoadingStatus
      label={label}
      className={cn("grid grid-cols-1 gap-3 md:grid-cols-2", className)}
    >
      {Array.from({ length: count }, (_, card) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: placeholder cards are positional and never reorder
          key={card}
          className="flex gap-3 rounded-md border border-border-1 bg-bg-panel p-4"
        >
          <Skeleton className="size-9 shrink-0 rounded-md" />
          <div className="flex min-w-0 flex-1 flex-col gap-2 pt-1">
            <Skeleton className="h-3.5 w-1/2" />
            <Skeleton className="h-3 w-3/4" />
            <Skeleton className="h-3 w-1/3" />
          </div>
        </div>
      ))}
    </LoadingStatus>
  );
}

// DetailSkeleton stands in for a detail page: the back link, the title row,
// and the first card of facts.
export function DetailSkeleton({
  label,
  className,
}: {
  label?: string;
  className?: string;
}) {
  return (
    <LoadingStatus
      label={label}
      className={cn("flex flex-col gap-5", className)}
    >
      <Skeleton className="h-3.5 w-20" />
      <div className="flex items-center gap-3">
        <Skeleton className="size-10 rounded-md" />
        <div className="flex flex-col gap-2">
          <Skeleton className="h-5 w-56" />
          <Skeleton className="h-3.5 w-36" />
        </div>
      </div>
      <div className="rounded-md border border-border-1 bg-bg-panel p-5">
        <div className="flex flex-col gap-3">
          <Skeleton className="h-3.5 w-2/5" />
          <Skeleton className="h-3.5 w-1/4" />
          <Skeleton className="h-3.5 w-1/3" />
          <Skeleton className="h-3.5 w-1/5" />
        </div>
      </div>
    </LoadingStatus>
  );
}

// StatSkeleton stands in for a row of `count` stats: label, value, and the
// sparkline band under them.
export function StatSkeleton({
  count = 4,
  label,
  className,
}: {
  count?: number;
  label?: string;
  className?: string;
}) {
  return (
    <LoadingStatus
      label={label}
      className={cn(
        "rounded-md border border-border-1 bg-bg-panel p-5",
        className,
      )}
    >
      <div className="grid grid-cols-2 gap-x-6 gap-y-5 md:grid-cols-4">
        {Array.from({ length: count }, (_, stat) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: placeholder stats are positional and never reorder
          <div key={stat} className="flex flex-col gap-2">
            <Skeleton className="h-3 w-20" />
            <Skeleton className="h-6 w-16" />
            <Skeleton className="h-8 w-full" />
          </div>
        ))}
      </div>
    </LoadingStatus>
  );
}
