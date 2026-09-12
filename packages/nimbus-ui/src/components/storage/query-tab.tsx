import { Code2, Play, Plus, X } from "lucide-react";
import { useMemo, useState } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { useServerUrl } from "../../hooks/use-server-url";
import { CodeBlock } from "../code-block";
import { CopyChip } from "../copy-chip";
import { paginatedQueryCommand } from "../onboarding/next-action";
import { Select } from "../select";
import {
  compileDocumentQuery,
  type DocumentFilter,
  type DocumentOrder,
  FILTER_OP_LABEL,
  FILTER_OPS,
  type FilterOp,
  formatFilterValue,
  paginatedRequestBody,
  parseFilterValue,
} from "./table-query";

/**
 * The Query tab of a table: the same filter set and sort the document grid
 * runs, as a form instead of chips, with the request it compiles to shown
 * as code. Run hands the query to the Documents tab through the URL, so
 * the grid, the pager, and the code here all read one definition
 * (`compileDocumentQuery`).
 *
 * An unindexed sort is an unbounded scan (DESIGN.md:269). The builder marks
 * each field as indexed or scan, and Run stays disabled behind an explicit
 * "scan anyway" until the operator has said so.
 */
export function QueryTab({
  tenant,
  table,
  fields,
  indexBacked,
  filters,
  order,
  onRun,
}: {
  tenant: string;
  table: string;
  fields: string[];
  indexBacked: Set<string>;
  filters: DocumentFilter[];
  order: DocumentOrder | null;
  onRun: (filters: DocumentFilter[], order: DocumentOrder | null) => void;
}) {
  const serverUrl = useServerUrl();
  const [rows, setRows] = useState<FilterRow[]>(() =>
    filters.map((filter, index) => ({
      key: index,
      field: filter.field,
      op: filter.op,
      value: formatFilterValue(filter.value),
    })),
  );
  const [nextKey, setNextKey] = useState(filters.length);
  const [sortField, setSortField] = useState(order?.field ?? NO_SORT);
  const [sortDir, setSortDir] = useState<"asc" | "desc">(
    order?.direction ?? "asc",
  );
  const [scanAnyway, setScanAnyway] = useState(false);
  const [showCode, setShowCode] = useState(false);

  const fieldOptions = useMemo(
    () =>
      fields.map((field) => ({
        value: field,
        label: indexBacked.has(field)
          ? `${field} (indexed)`
          : `${field} (scan)`,
      })),
    [fields, indexBacked],
  );

  const compiledFilters = useMemo<DocumentFilter[]>(
    () =>
      rows
        .filter((row) => row.field !== "")
        .map((row) => ({
          field: row.field,
          op: row.op,
          value: parseFilterValue(row.value),
        })),
    [rows],
  );
  const compiledOrder = useMemo<DocumentOrder | null>(
    () =>
      sortField === NO_SORT ? null : { field: sortField, direction: sortDir },
    [sortField, sortDir],
  );
  const scanSort = compiledOrder !== null && !indexBacked.has(sortField);
  const canRun = !scanSort || scanAnyway;

  const request = useMemo(
    () =>
      paginatedRequestBody(
        compileDocumentQuery(table, compiledFilters, compiledOrder),
      ),
    [table, compiledFilters, compiledOrder],
  );
  const body = useMemo(() => JSON.stringify(request, null, 2), [request]);
  const command = useMemo(
    () =>
      paginatedQueryCommand({
        serverUrl,
        tenant,
        body: JSON.stringify(request),
      }),
    [serverUrl, tenant, request],
  );

  const addRow = () => {
    setRows((prev) => [
      ...prev,
      { key: nextKey, field: fields[0] ?? "", op: "eq", value: "" },
    ]);
    setNextKey((key) => key + 1);
  };
  const patchRow = (key: number, patch: Partial<FilterRow>) =>
    setRows((prev) =>
      prev.map((row) => (row.key === key ? { ...row, ...patch } : row)),
    );
  const removeRow = (key: number) =>
    setRows((prev) => prev.filter((row) => row.key !== key));

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto rounded-md border border-border-1 bg-bg-panel p-4"
      data-testid="documents-query-tab"
    >
      <p className="max-w-prose text-sm text-text-3">
        Build the query the Documents tab runs. Each row narrows the result; the
        sort orders it. A field marked scan has no index, so sorting on it reads
        the whole table.
      </p>

      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1 text-xs font-medium text-text-3">
          Filters
        </legend>
        {rows.length === 0 ? (
          <p
            className="text-xs text-text-3"
            data-testid="documents-query-no-filters"
          >
            No filters: every document in {table}.
          </p>
        ) : null}
        {rows.map((row) => (
          <div
            key={row.key}
            className="flex flex-wrap items-center gap-2"
            data-testid={`documents-query-row-${row.key}`}
          >
            <Select
              label="field"
              value={row.field}
              options={fieldOptions}
              onChange={(field) => patchRow(row.key, { field })}
              testid={`documents-query-field-${row.key}`}
            />
            <Select
              label="op"
              value={row.op}
              options={FILTER_OPS.map((op) => ({
                value: op,
                label: `${FILTER_OP_LABEL[op]}  ${op}`,
              }))}
              onChange={(op) => patchRow(row.key, { op })}
              testid={`documents-query-op-${row.key}`}
            />
            <input
              value={row.value}
              onChange={(event) =>
                patchRow(row.key, { value: event.target.value })
              }
              // Documents are JSON: `42` filters a number, `"42"` a string.
              placeholder='value: 42, true, null, "text"'
              aria-label="Filter value"
              className={INPUT_CLASS}
              data-testid={`documents-query-value-${row.key}`}
            />
            <button
              type="button"
              className="rounded-xs p-1 text-text-3 hover:text-text-1"
              aria-label="Remove filter"
              onClick={() => removeRow(row.key)}
              data-testid={`documents-query-remove-${row.key}`}
            >
              <X className="size-3.5" aria-hidden />
            </button>
          </div>
        ))}
        <div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={addRow}
            disabled={fields.length === 0}
            data-testid="documents-query-add-row"
          >
            <Plus className="size-3.5" aria-hidden /> Add filter
          </Button>
        </div>
      </fieldset>

      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1 text-xs font-medium text-text-3">Sort</legend>
        <div className="flex flex-wrap items-center gap-2">
          <Select
            label="field"
            value={sortField}
            options={[
              { value: NO_SORT, label: "natural order (_id)" },
              ...fieldOptions.filter((option) => option.value !== "_id"),
            ]}
            onChange={(field) => {
              setSortField(field);
              setScanAnyway(false);
            }}
            testid="documents-query-sort-field"
          />
          <Select
            label="direction"
            value={sortDir}
            options={[
              { value: "asc", label: "ascending" },
              { value: "desc", label: "descending" },
            ]}
            onChange={setSortDir}
            testid="documents-query-sort-dir"
          />
        </div>
        {scanSort ? (
          <div
            className="flex items-center gap-2 rounded-md border border-warning/40 bg-warning-tint/40 px-3 py-2 text-xs text-text-1"
            data-testid="documents-query-scan-warning"
          >
            <Checkbox
              id="documents-query-scan-anyway"
              checked={scanAnyway}
              onCheckedChange={(checked) => setScanAnyway(checked === true)}
              data-testid="documents-query-scan-anyway"
            />
            <label htmlFor="documents-query-scan-anyway">
              No index leads with <span className="font-mono">{sortField}</span>
              ; the sort reads the whole table. Scan anyway.
            </label>
          </div>
        ) : null}
      </fieldset>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          size="sm"
          disabled={!canRun}
          onClick={() => onRun(compiledFilters, compiledOrder)}
          data-testid="documents-query-run"
        >
          <Play className="size-3.5" aria-hidden /> Run
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-pressed={showCode}
          onClick={() => setShowCode((open) => !open)}
          data-testid="documents-query-code-toggle"
        >
          <Code2 className="size-3.5" aria-hidden />{" "}
          {showCode ? "Hide code" : "Show as code"}
        </Button>
      </div>

      {showCode ? (
        <div className="flex flex-col gap-2" data-testid="documents-query-code">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-text-3">
              Request body for{" "}
              <span className="font-mono">
                POST /api/tenants/{tenant}/query/paginated
              </span>
              , the same body the Documents tab sends.
            </span>
            <CopyChip
              label="request body"
              value={body}
              testid="documents-query-copy-body"
            >
              copy body
            </CopyChip>
          </div>
          <CodeBlock
            code={body}
            lang="json"
            testid="documents-query-code-body"
          />
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-text-3">As a shell command.</span>
            <CopyChip
              label="curl command"
              value={command}
              testid="documents-query-copy-curl"
            >
              copy curl
            </CopyChip>
          </div>
          <CodeBlock
            code={command}
            lang="bash"
            testid="documents-query-code-curl"
          />
        </div>
      ) : null}
    </div>
  );
}

type FilterRow = { key: number; field: string; op: FilterOp; value: string };

/** Select value for "no sort"; never a real field name. */
const NO_SORT = " natural";

const INPUT_CLASS =
  "h-[26px] w-56 rounded-xs border border-border-2 bg-bg-panel px-2 font-mono text-xs text-text-1 placeholder:text-text-3 focus-visible:border-accent-edge";
