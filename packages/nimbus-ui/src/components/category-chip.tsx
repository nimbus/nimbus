import { cn } from "../lib/cn";

/**
 * A categorical badge: function kind (`Query`, `Mutation`, `Action`, `HTTP`,
 * `Scheduled`, `Cron`), adapter (`Convex`, `MongoDB`, …), or storage backend
 * (`redb`, `SQLite`, …).
 *
 * DESIGN.md splits badges in two. A *state* is a labeled dot whose colour is
 * bound to a fixed token table (`StateChip`). A *category* is a filled pill —
 * it names what a thing is, not how it is doing, so it carries no health
 * colour. Routing a category through `StateChip` is the bug this component
 * exists to prevent: `kind: "query"` matches no state, falls through to
 * `unknown`, and renders a literal `?` next to the label.
 *
 * The fill stays neutral on purpose. DESIGN.md fixes a colour mapping for
 * states and deliberately gives none for categories; inventing one would turn
 * a calm row into the pill farm the spec warns against.
 */
export function CategoryChip({
  value,
  className,
}: {
  value: string | null | undefined;
  className?: string;
}) {
  const label = value && value.length > 0 ? value : "unknown";
  return (
    <span
      className={cn(
        "inline-flex items-center rounded border border-app bg-surface-2 px-1.5 py-0.5",
        // `leading-none` pins the pill to a known 17px so a row of pills and
        // a row of 8px state dots occupy the same reserved line.
        "font-mono text-xs leading-none uppercase tracking-wide text-muted",
        className,
      )}
      data-category={label.toLowerCase()}
    >
      {label}
    </span>
  );
}
