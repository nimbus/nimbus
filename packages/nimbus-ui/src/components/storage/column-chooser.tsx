import { ChevronDown, ChevronUp, Columns3 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";

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
          "flex h-[26px] items-center gap-1 rounded-xs border border-border-2 px-2 text-xs font-medium hover:bg-bg-panel",
          open ? "bg-bg-panel text-text-1" : "text-text-3 hover:text-text-1",
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
          className="absolute right-0 top-full z-30 mt-1 flex max-h-80 w-72 flex-col rounded-md border border-border-2 bg-bg-panel shadow-lg"
          data-testid="documents-column-chooser-panel"
        >
          <div className="border-b border-border-2 px-3 py-2">
            <p className="text-xs font-medium text-text-3">
              {fromSchema ? "schema fields" : "fields seen so far"}
            </p>
            {fromSchema ? null : (
              <p className="mt-1 text-xs leading-snug text-text-3">
                This table has no schema, so the field list grows as you page
                through documents.
              </p>
            )}
          </div>
          <ul className="min-h-0 flex-1 overflow-auto py-1">
            {available.length === 0 ? (
              <li className="px-3 py-3 font-mono text-xs text-text-3">
                No fields discovered yet.
              </li>
            ) : null}
            {available.map((field) => {
              const index = visible.indexOf(field);
              const isVisible = index >= 0;
              return (
                <li
                  key={field}
                  className="flex h-8 items-center gap-2 px-3 hover:bg-bg-raised"
                >
                  <Checkbox
                    aria-label={`Show column ${field}`}
                    checked={isVisible}
                    onCheckedChange={(checked) => onToggle(field, checked)}
                    data-testid={`documents-column-toggle-${field}`}
                  />
                  <span
                    className={cn(
                      "flex-1 truncate font-mono text-xs",
                      isVisible ? "text-text-1" : "text-text-3",
                    )}
                  >
                    {field}
                  </span>
                  {/*
                    DESIGN.md:877 — 32px square. The row is a fixed `h-8`, so
                    the box fills the row's full height without growing it, and
                    the glyph is centred rather than padded: padding inside a
                    fixed box shrinks the glyph's cell instead of widening the
                    target.
                  */}
                  <button
                    type="button"
                    aria-label={`Move ${field} left`}
                    disabled={!isVisible || index <= 1}
                    onClick={() => onMove(field, -1)}
                    className="flex h-8 w-8 items-center justify-center rounded-xs text-text-3 hover:text-text-1 disabled:opacity-30"
                  >
                    <ChevronUp size={14} aria-hidden />
                  </button>
                  <button
                    type="button"
                    aria-label={`Move ${field} right`}
                    disabled={!isVisible || index === visible.length - 1}
                    onClick={() => onMove(field, 1)}
                    className="flex h-8 w-8 items-center justify-center rounded-xs text-text-3 hover:text-text-1 disabled:opacity-30"
                  >
                    <ChevronDown size={14} aria-hidden />
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="flex items-center justify-between border-t border-border-2 px-3 py-2">
            <span className="font-mono text-xs text-text-3">
              saved per table
            </span>
            <button
              type="button"
              onClick={onReset}
              className="rounded-xs border border-border-2 px-2 py-0.5 text-xs font-medium text-text-3 hover:bg-bg-raised hover:text-text-1"
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
