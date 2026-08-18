import { ChevronDown, ChevronUp, Columns3 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { cn } from "../../lib/cn";
import { Checkbox } from "../checkbox";

/**
 * Column visibility and order for the document browser.
 *
 * The field list is honest about where it came from. With a schema it is the
 * declared fields. Without one it is only the fields seen in the pages visited
 * so far — the previous browser derived columns from the first 25 documents and
 * silently capped them at 8, which is how an operator concludes a field does
 * not exist.
 */
export function ColumnChooser({
  available,
  visible,
  fromSchema,
  onToggle,
  onMove,
  onReset,
}: {
  /** Every field known for this table, in discovery order. Excludes `_id`. */
  available: string[];
  /** Currently rendered columns, in render order. Includes `_id` first. */
  visible: string[];
  fromSchema: boolean;
  onToggle: (field: string, visible: boolean) => void;
  onMove: (field: string, delta: number) => void;
  onReset: () => void;
}) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      if (!wrapRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const shownCount = visible.length - 1; // `_id` is pinned, not chooseable.
  const hiddenCount = available.length - shownCount;

  return (
    <div className="relative shrink-0" ref={wrapRef}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-haspopup="dialog"
        className={cn(
          "flex h-[26px] items-center gap-1 rounded border border-app px-2 font-mono text-xs uppercase tracking-wide hover:bg-surface",
          open ? "bg-surface text-default" : "text-muted hover:text-default",
        )}
        data-testid="documents-column-chooser"
      >
        <Columns3 size={11} aria-hidden />
        columns {shownCount}/{available.length}
        {hiddenCount > 0 ? (
          <span className="text-warning" data-testid="documents-columns-hidden">
            +{hiddenCount} hidden
          </span>
        ) : null}
      </button>

      {open ? (
        <div
          className="absolute right-0 top-full z-30 mt-1 flex max-h-80 w-72 flex-col rounded-md border border-app bg-surface shadow-lg"
          data-testid="documents-column-chooser-panel"
        >
          <div className="border-b border-app px-3 py-2">
            <p className="font-mono text-xs uppercase tracking-[0.14em] text-muted">
              {fromSchema ? "schema fields" : "fields seen so far"}
            </p>
            {fromSchema ? null : (
              <p className="mt-1 text-xs leading-snug text-muted">
                This table has no schema, so the field list grows as you page
                through documents.
              </p>
            )}
          </div>
          <ul className="min-h-0 flex-1 overflow-auto py-1">
            {available.length === 0 ? (
              <li className="px-3 py-3 font-mono text-xs text-muted">
                No fields discovered yet.
              </li>
            ) : null}
            {available.map((field) => {
              const index = visible.indexOf(field);
              const isVisible = index >= 0;
              return (
                <li
                  key={field}
                  className="flex h-8 items-center gap-2 px-3 hover:bg-surface-2"
                >
                  <Checkbox
                    label={`Show column ${field}`}
                    checked={isVisible}
                    onChange={(checked) => onToggle(field, checked)}
                    testid={`documents-column-toggle-${field}`}
                  />
                  <span
                    className={cn(
                      "flex-1 truncate font-mono text-xs",
                      isVisible ? "text-default" : "text-muted",
                    )}
                  >
                    {field}
                  </span>
                  <button
                    type="button"
                    aria-label={`Move ${field} left`}
                    disabled={!isVisible || index <= 1}
                    onClick={() => onMove(field, -1)}
                    className="text-muted hover:text-default disabled:opacity-30"
                  >
                    <ChevronUp size={12} aria-hidden />
                  </button>
                  <button
                    type="button"
                    aria-label={`Move ${field} right`}
                    disabled={!isVisible || index === visible.length - 1}
                    onClick={() => onMove(field, 1)}
                    className="text-muted hover:text-default disabled:opacity-30"
                  >
                    <ChevronDown size={12} aria-hidden />
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="flex items-center justify-between border-t border-app px-3 py-2">
            <span className="font-mono text-xs text-muted">
              saved per table
            </span>
            <button
              type="button"
              onClick={onReset}
              className="rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface-2 hover:text-default"
              data-testid="documents-column-reset"
            >
              reset
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
