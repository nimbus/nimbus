import { useCallback, useMemo, useState } from "react";

import { DataTable, dataColumns } from "../../../components/data-table";
import { EmptyState } from "../../../components/empty-state";
import { FacetBar, FacetButton } from "../../../components/facet-bar";
import { LoadFailed } from "../../../components/load-failed";
import { CategoryPill } from "../../../components/pill";
import { RelativeTime } from "../../../components/time";
import { useApiRead } from "../../../hooks/use-api-read";
import { formatCount } from "../../../lib/format";
import {
  type ObservabilityTabProps,
  SystemLensButton,
  TenantFacet,
} from "./-facets";
import type { ErrorGroup, ErrorGroupPage } from "./-types";

export const ERROR_GROUP_LIMIT = 200;

// The address of the error-group read for one tenant scope.
export function errorGroupsPath(tenantId: string | null): string {
  const params = new URLSearchParams();
  if (tenantId) params.set("tenant", tenantId);
  params.set("limit", String(ERROR_GROUP_LIMIT));
  return `/api/console/errors?${params.toString()}`;
}

const groupCol = dataColumns<ErrorGroup>();

const GROUP_COLUMNS = [
  groupCol.accessor("lastSeen", {
    header: "Last seen",
    size: 110,
    cell: (ctx) => <RelativeTime epochMs={ctx.getValue()} />,
  }),
  groupCol.accessor("count", {
    header: () => <span className="block text-right">Runs</span>,
    size: 72,
    cell: (ctx) => (
      <span className="block text-right font-mono text-xs tabular text-text-1">
        {formatCount(ctx.getValue())}
      </span>
    ),
  }),
  groupCol.accessor("functionPath", {
    header: "Function",
    size: 200,
    cell: (ctx) => (
      <span
        className="block truncate font-mono text-xs text-text-1"
        title={ctx.getValue()}
      >
        {ctx.getValue()}
      </span>
    ),
  }),
  groupCol.accessor("class", {
    header: "Class",
    size: 140,
    cell: (ctx) => <CategoryPill value={ctx.getValue()} />,
  }),
  groupCol.accessor("message", {
    header: "Message",
    size: 360,
    cell: (ctx) => {
      const location = ctx.row.original.location;
      return (
        <span className="flex min-w-0 items-baseline gap-2">
          <span
            className="min-w-0 truncate font-mono text-xs text-text-1"
            title={ctx.getValue()}
          >
            {ctx.getValue()}
          </span>
          {location ? (
            <span className="shrink-0 font-mono text-xs text-text-3">
              {location}
            </span>
          ) : null}
        </span>
      );
    },
  }),
  groupCol.accessor("firstSeen", {
    header: "First seen",
    size: 110,
    cell: (ctx) => <RelativeTime epochMs={ctx.getValue()} />,
  }),
  groupCol.accessor("tenantId", {
    header: "Tenant",
    size: 110,
    cell: (ctx) => (
      <span className="block truncate font-mono text-xs text-text-3">
        {ctx.getValue()}
      </span>
    ),
  }),
];

// ErrorsTab is the failed runs folded by fingerprint: one row per function,
// error class, and message, with how many runs share it and when it was
// first and last seen. A row drills into the Runs tab narrowed to that
// group, where each run has its sheet, its logs, and its trace.
export function ErrorsTab({
  tenantId,
  allowAllTenants,
  setSearch,
  setSearchAction,
}: ObservabilityTabProps) {
  // The read is one-shot; `attempt` re-keys it so the reader can refresh
  // the groups without leaving the tab.
  const [attempt, setAttempt] = useState(0);
  const path = useMemo(
    () => `${errorGroupsPath(tenantId)}&attempt=${attempt}`,
    [tenantId, attempt],
  );
  const page = useApiRead<ErrorGroupPage>(path);
  const refresh = useCallback(() => setAttempt((n) => n + 1), []);

  const groups = page.kind === "ok" ? page.value.groups : [];

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="observability-errors"
    >
      <FacetBar
        label="Error group facets"
        testid="observability-error-filters"
        trailing={
          <>
            <FacetButton onClick={refresh} testid="observability-error-refresh">
              refresh
            </FacetButton>
            <SystemLensButton />
          </>
        }
      >
        <TenantFacet
          tenantId={tenantId}
          allowAllTenants={allowAllTenants}
          setSearch={setSearch}
        />
        {page.kind === "ok" ? <ScanNote page={page.value} /> : null}
      </FacetBar>
      {page.kind === "error" ? (
        <LoadFailed
          what="error groups"
          error={page.message}
          onRetry={refresh}
          testid="observability-errors-failed"
        />
      ) : page.kind === "ok" && groups.length === 0 ? (
        <div className="min-h-0 flex-1 overflow-auto rounded-md border border-border-2 bg-bg-panel">
          <EmptyState
            title="No failed runs"
            body={
              tenantId
                ? `Every run the server has recorded for ${tenantId} succeeded. A run that throws lands here, folded with the runs that fail the same way.`
                : "Every run the server has recorded succeeded. A run that throws lands here, folded with the runs that fail the same way."
            }
            testid="observability-errors-empty"
          />
        </div>
      ) : (
        <DataTable
          columns={GROUP_COLUMNS}
          data={groups}
          getRowId={(row) => row.fingerprint}
          ariaLabel="Error groups"
          loading={page.kind === "loading"}
          onRowActivate={(row) =>
            setSearchAction({
              tab: "runs",
              fingerprint: row.fingerprint,
              status: undefined,
              functionPath: undefined,
              run: undefined,
            })
          }
          rowTestid={(row) => `observability-error-row-${row.fingerprint}`}
          testid="observability-errors-table"
          className="min-h-0 flex-1"
        />
      )}
    </div>
  );
}

// The groups fold the newest failed runs in a bounded window. When the
// window ended before the oldest failure, older groups may exist, and the
// note says so before the reader concludes the table is complete.
function ScanNote({ page }: { page: ErrorGroupPage }) {
  return (
    <span
      className="font-mono text-xs text-text-3"
      data-testid="observability-error-scan"
    >
      {page.exhaustive
        ? `${formatCount(page.scanned)} failed run${page.scanned === 1 ? "" : "s"} grouped`
        : `newest ${formatCount(page.scanned)} failed runs grouped; older failures are not shown`}
    </span>
  );
}
