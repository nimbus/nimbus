import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

// FacetBar is the filter strip above a list: a row of facets (a select, a
// text input, a switch) with an action cluster pinned to the end. It wraps
// rather than clips, because every page column is `overflow-hidden` and a
// strip that cannot compress pushes its trailing controls out of the
// viewport with no scrollbar to recover them.
export function FacetBar({
  children,
  trailing,
  label,
  testid,
  className,
}: {
  children: ReactNode;
  // The action cluster: switches, clear, the lens button. Pinned to the end
  // of the strip and wraps as one unit.
  trailing?: ReactNode;
  label: string;
  testid?: string;
  className?: string;
}) {
  return (
    <div
      role="toolbar"
      aria-label={label}
      className={cn("flex flex-wrap items-center gap-2", className)}
      data-testid={testid}
    >
      {children}
      {trailing ? (
        <div className="ml-auto flex flex-wrap items-center gap-2">
          {trailing}
        </div>
      ) : null}
    </div>
  );
}

// FacetInput is a labelled free-text facet. It is bounded (`w-[14ch]`, or
// `w-[26ch]` for a `wide` facet such as a search field, and `min-w-0`) so
// the bar cannot be widened past its container by the input's intrinsic
// size.
export function FacetInput({
  id,
  label,
  value,
  placeholder,
  onChange,
  wide = false,
  testid,
}: {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
  wide?: boolean;
  testid?: string;
}) {
  return (
    <label
      htmlFor={id}
      className="flex min-w-0 items-center gap-1.5 text-xs font-medium text-text-3"
    >
      <span className="shrink-0">{label}</span>
      <input
        id={id}
        type="text"
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        className={cn(
          "min-w-0 rounded-xs border border-border-2 bg-bg-panel px-2 py-1 font-mono text-xs text-text-1 placeholder:text-text-3 focus-visible:border-accent-edge",
          wide ? "w-[26ch]" : "w-[14ch]",
        )}
        data-testid={testid}
      />
    </label>
  );
}

// FacetToggle is a boolean facet: a switch that reads as a chip and reports
// its state through `aria-checked`.
export function FacetToggle({
  id,
  label,
  value,
  onChange,
  testid,
}: {
  id?: string;
  label: string;
  value: boolean;
  onChange: (value: boolean) => void;
  testid?: string;
}) {
  return (
    <button
      type="button"
      id={id}
      role="switch"
      aria-checked={value}
      onClick={() => onChange(!value)}
      className={cn(
        "rounded-xs border px-2 py-1 text-xs font-medium",
        value
          ? "border-border-3 bg-bg-panel text-text-1"
          : "border-border-2 text-text-3 hover:bg-bg-panel hover:text-text-1",
      )}
      data-testid={testid}
    >
      {label}
    </button>
  );
}

// FacetButton is a bar-sized action: clear the facets, resume a paused
// stream, open the lens. `danger` is for the one that reports a stopped
// state (a paused stream) and needs to be seen.
export function FacetButton({
  children,
  onClick,
  tone = "default",
  title,
  testid,
  className,
}: {
  children: ReactNode;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  tone?: "default" | "danger";
  title?: string;
  testid?: string;
  className?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={cn(
        "rounded-xs border px-2 py-1 text-xs font-medium",
        tone === "danger"
          ? "border-error text-error hover:bg-bg-raised"
          : "border-border-2 text-text-3 hover:bg-bg-panel hover:text-text-1",
        className,
      )}
      data-testid={testid}
    >
      {children}
    </button>
  );
}
