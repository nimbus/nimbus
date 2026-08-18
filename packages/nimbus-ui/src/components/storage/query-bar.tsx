import { Plus, X } from "lucide-react";
import { type ReactNode, useState } from "react";

import { cn } from "../../lib/cn";
import { Select } from "../select";
import {
  type DocumentFilter,
  type DocumentOrder,
  describeFilter,
  FILTER_OP_LABEL,
  FILTER_OPS,
  type FilterOp,
  parseFilterValue,
} from "./table-query";

/**
 * The document browser's query surface: active filters and sort order as
 * editable chips (DESIGN.md:948), plus the add-filter editor and the
 * unbounded-scan guard (DESIGN.md:269).
 *
 * Everything here writes to the URL, so a filtered view is shareable and
 * survives a refresh.
 */
export function QueryBar({
  fields,
  filters,
  order,
  indexBacked,
  pendingScanSort,
  onFiltersChange,
  onOrderChange,
  onConfirmScanSort,
  onCancelScanSort,
  trailing,
}: {
  fields: string[];
  filters: DocumentFilter[];
  order: DocumentOrder | null;
  indexBacked: Set<string>;
  /** Field awaiting an explicit "scan anyway" before it becomes the sort. */
  pendingScanSort: string | null;
  onFiltersChange: (next: DocumentFilter[]) => void;
  onOrderChange: (next: DocumentOrder | null) => void;
  onConfirmScanSort: () => void;
  onCancelScanSort: () => void;
  /** Column chooser slot, right-aligned. */
  trailing?: ReactNode;
}) {
  const [adding, setAdding] = useState(false);

  return (
    <div
      className="flex shrink-0 flex-col gap-2 border-b border-app bg-surface-2 px-3 py-2"
      data-testid="documents-query-bar"
    >
      <div className="flex items-center gap-2">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
          {filters.map((filter, index) => (
            <Chip
              // biome-ignore lint/suspicious/noArrayIndexKey: filters are positional, may repeat a field, and are removed by index
              key={`${filter.field}-${filter.op}-${index}`}
              testid={`documents-filter-chip-${filter.field}`}
              label={describeFilter(filter)}
              removeLabel={`Remove filter ${describeFilter(filter)}`}
              onRemove={() =>
                onFiltersChange(filters.filter((_, i) => i !== index))
              }
            />
          ))}
          {order ? (
            <Chip
              testid="documents-sort-chip"
              label={`sort ${order.field} ${order.direction === "asc" ? "↑" : "↓"}`}
              removeLabel="Clear sort"
              warn={!indexBacked.has(order.field)}
              onRemove={() => onOrderChange(null)}
            />
          ) : null}
          {filters.length === 0 && !order ? (
            <span className="font-mono text-xs text-muted">
              no filters · natural order
            </span>
          ) : null}
        </div>
        <button
          type="button"
          onClick={() => setAdding((v) => !v)}
          aria-expanded={adding}
          className={cn(
            "flex h-[26px] shrink-0 items-center gap-1 rounded border border-app px-2 font-mono text-xs uppercase tracking-wide hover:bg-surface",
            adding
              ? "bg-surface text-default"
              : "text-muted hover:text-default",
          )}
          data-testid="documents-add-filter"
        >
          <Plus size={11} aria-hidden />
          filter
        </button>
        {trailing}
      </div>

      {adding ? (
        <FilterEditor
          fields={fields}
          onCancel={() => setAdding(false)}
          onAdd={(filter) => {
            onFiltersChange([...filters, filter]);
            setAdding(false);
          }}
        />
      ) : null}

      {pendingScanSort ? (
        <div
          className="flex flex-wrap items-center gap-2 rounded border border-warning/40 bg-surface px-2 py-1.5 font-mono text-xs text-default"
          data-testid="documents-scan-warning"
          role="alert"
        >
          <span>
            <span className="text-warning">unindexed sort</span> —{" "}
            <span className="text-muted">
              no index leads with{" "}
              <b className="text-default">{pendingScanSort}</b>, so the engine
              sorts the whole filtered table in memory.
            </span>
          </span>
          <button
            type="button"
            onClick={onConfirmScanSort}
            className="rounded border border-app px-2 py-0.5 uppercase tracking-wide text-warning hover:bg-surface-2"
            data-testid="documents-scan-confirm"
          >
            scan anyway
          </button>
          <button
            type="button"
            onClick={onCancelScanSort}
            className="rounded border border-app px-2 py-0.5 uppercase tracking-wide text-muted hover:bg-surface-2 hover:text-default"
            data-testid="documents-scan-cancel"
          >
            cancel
          </button>
        </div>
      ) : null}
    </div>
  );
}

function Chip({
  label,
  removeLabel,
  onRemove,
  warn,
  testid,
}: {
  label: string;
  removeLabel: string;
  onRemove: () => void;
  warn?: boolean;
  testid?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-[22px] max-w-[32ch] items-center gap-1 rounded border px-1.5 font-mono text-xs",
        warn ? "border-warning/40 text-warning" : "border-app text-default",
      )}
      data-testid={testid}
    >
      <span className="truncate">{label}</span>
      <button
        type="button"
        aria-label={removeLabel}
        onClick={onRemove}
        className="text-muted hover:text-default"
      >
        <X size={11} aria-hidden />
      </button>
    </span>
  );
}

function FilterEditor({
  fields,
  onAdd,
  onCancel,
}: {
  fields: string[];
  onAdd: (filter: DocumentFilter) => void;
  onCancel: () => void;
}) {
  const [field, setField] = useState(fields[0] ?? "");
  const [op, setOp] = useState<FilterOp>("eq");
  const [value, setValue] = useState("");

  const submit = () => {
    if (!field) return;
    onAdd({ field, op, value: parseFilterValue(value) });
    setValue("");
  };

  return (
    <form
      className="flex flex-wrap items-center gap-2"
      data-testid="documents-filter-editor"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <Select
        label="field"
        value={field}
        options={fields.map((f) => ({ value: f, label: f }))}
        onChange={setField}
        testid="documents-filter-field"
      />
      <Select
        label="op"
        value={op}
        options={FILTER_OPS.map((o) => ({
          value: o,
          label: `${FILTER_OP_LABEL[o]}  ${o}`,
        }))}
        onChange={setOp}
        testid="documents-filter-op"
      />
      <input
        value={value}
        onChange={(event) => setValue(event.target.value)}
        // Documents are JSON: `42` filters a number, `"42"` a string.
        placeholder='value — 42, true, null, "text"'
        aria-label="Filter value"
        className="h-[26px] w-56 rounded border border-app bg-surface px-2 font-mono text-xs text-default placeholder:text-muted focus:outline-none focus-visible:border-strong"
        data-testid="documents-filter-value"
      />
      <button
        type="submit"
        className="h-[26px] rounded border border-app px-2 font-mono text-xs uppercase tracking-wide text-default hover:bg-surface"
        data-testid="documents-filter-apply"
      >
        apply
      </button>
      <button
        type="button"
        onClick={onCancel}
        className="h-[26px] rounded border border-app px-2 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
      >
        cancel
      </button>
    </form>
  );
}
