import { Kbd } from "@/components/ui/kbd";

/**
 * The bulk action toolbar (DESIGN.md:1120-1123).
 *
 * Bulk delete is the highest-stakes interaction on this screen, and it used to
 * report itself only as a `(n)` suffix on a small button in the far
 * top-right corner. The count, the destructive action, and the way out belong
 * together, directly above the rows they act on.
 */
export function BulkToolbar({
  count,
  onDelete,
  onClear,
}: {
  count: number;
  onDelete: () => void;
  onClear: () => void;
}) {
  return (
    <div
      className="flex shrink-0 items-center gap-3 border-b border-border-2 bg-bg-raised px-3 py-2"
      data-testid="documents-bulk-toolbar"
      role="toolbar"
      aria-label="Bulk document actions"
    >
      <span className="font-mono text-xs text-text-1">
        <span className="tabular">{count}</span> selected
      </span>
      <button
        type="button"
        onClick={onDelete}
        className="rounded-xs border border-border-2 px-2 py-0.5 text-xs font-medium text-error hover:bg-bg-panel"
        data-testid="documents-bulk-delete"
      >
        delete
      </button>
      <button
        type="button"
        onClick={onClear}
        className="rounded-xs border border-border-2 px-2 py-0.5 text-xs font-medium text-text-3 hover:bg-bg-panel hover:text-text-1"
        data-testid="documents-bulk-clear"
      >
        clear
      </button>
      <span className="ml-auto flex items-center gap-1.5 text-xs font-medium text-text-3">
        <Kbd>⎋</Kbd> clears
      </span>
    </div>
  );
}
