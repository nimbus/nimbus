import { ChevronDown, ChevronUp, Columns3 } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

/**
 * Column visibility and order for the document browser, in a popover.
 *
 * The field list is honest about where it came from. With a schema it is the
 * declared fields. Without one it is only the fields seen in the pages visited
 * so far — a list capped at the first page is how an operator concludes a
 * field does not exist.
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
  const shownCount = visible.length - 1; // `_id` is pinned, not chooseable.
  const hiddenCount = available.length - shownCount;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button
            type="button"
            variant="outline"
            size="sm"
            data-testid="documents-column-chooser"
          />
        }
      >
        <Columns3 aria-hidden />
        Columns {shownCount}/{available.length}
        {hiddenCount > 0 ? (
          <span className="text-warning" data-testid="documents-columns-hidden">
            +{hiddenCount} hidden
          </span>
        ) : null}
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="flex max-h-80 w-72 flex-col gap-0 border border-border-2 bg-bg-panel p-0 text-text-1"
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
                {/* 32px square targets that fill the fixed row height. */}
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
          <span className="font-mono text-xs text-text-3">saved per table</span>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            onClick={onReset}
            data-testid="documents-column-reset"
          >
            Reset
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
