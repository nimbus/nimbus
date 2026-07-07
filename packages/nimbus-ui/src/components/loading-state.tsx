import { cn } from "../lib/cn";

// Full-panel loading placeholder: a single centered status line that fills its
// container. The sibling `LoadingCell` handles inline value-cell loading via a
// `LoadingValue<T>` switch; this is the panel-sized counterpart the routes
// reach for while a whole table or tab is still resolving.
export function LoadingState({
  label,
  testid,
  className,
}: {
  label: string;
  testid?: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex h-full items-center justify-center font-mono text-xs text-muted",
        className,
      )}
      data-testid={testid}
    >
      {label}
    </div>
  );
}
