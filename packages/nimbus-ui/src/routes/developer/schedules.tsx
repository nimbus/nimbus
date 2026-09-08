import { useQuery } from "@nimbus/nimbus/react";
import {
  createFileRoute,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { Ellipsis } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { api } from "../../../convex/_generated/api";
import { ConfirmDialog } from "../../components/confirm-dialog";
import {
  DataTable,
  dataColumns,
  type RowAnchor,
} from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import {
  createCronCommand,
  scheduleJobCommand,
} from "../../components/onboarding/next-action";
import { PageHeader } from "../../components/page-header";
import { StatePill } from "../../components/pill";
import {
  RowContextMenu,
  type RowMenuItem,
} from "../../components/storage/row-context-menu";
import { RelativeTime } from "../../components/time";
import { Button } from "../../components/ui/button";
import { useServerUrl } from "../../hooks/use-server-url";
import { shortId } from "../../lib/format";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
import { useUiStore } from "../../store/ui-store";
import { ScheduleSheet, type SheetTarget } from "./schedules/-schedule-sheet";
import {
  type CronJobDoc,
  formatSchedule,
  parseSchedulesSearch,
  type ScheduledJobDoc,
  type ScheduleSection,
  type SchedulesSearch,
} from "./schedules/-types";
import { useScheduleActions } from "./schedules/-use-schedule-actions";

export const Route = createFileRoute("/developer/schedules")({
  validateSearch: parseSchedulesSearch,
  component: SchedulesPage,
});

export const SCHEDULES_SUB_PANEL: SubPanelSpec = {
  kind: "static",
  title: "Schedules",
  items: [
    {
      id: "scheduled",
      label: "Scheduled",
      to: "/developer/schedules",
      search: { section: "scheduled" },
    },
    {
      id: "cron",
      label: "Cron",
      to: "/developer/schedules",
      search: { section: "cron" },
    },
  ],
};

// A row menu anchored on one row of either table.
type JobMenu = RowAnchor & { kind: "job"; row: ScheduledJobDoc };
type CronMenu = RowAnchor & { kind: "cron"; row: CronJobDoc };
type MenuState = JobMenu | CronMenu;

function SchedulesPage() {
  useContributeSubPanel(SCHEDULES_SUB_PANEL);
  const search = useSearch({ from: "/developer/schedules" });
  const navigate = useNavigate();
  const section: ScheduleSection = search.section ?? "scheduled";
  const activeTenant = useUiStore((s) => s.activeTenant);
  const actions = useScheduleActions();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [confirmCron, setConfirmCron] = useState<CronJobDoc | null>(null);

  const scheduled = useQuery(api.scheduled_jobs.list, {
    tenantId: activeTenant,
    status: null,
    limit: 200,
  }) as ScheduledJobDoc[] | undefined;

  const cron = useQuery(api.cron_jobs.list, {
    tenantId: activeTenant,
    status: null,
    limit: 200,
  }) as CronJobDoc[] | undefined;

  const setSearch = useCallback(
    (patch: Partial<SchedulesSearch>) =>
      navigate({
        to: "/developer/schedules",
        search: (prev) => ({ ...parseSchedulesSearch(prev), ...patch }),
        replace: true,
      }),
    [navigate],
  );

  const target: SheetTarget | undefined = search.job
    ? { kind: "job", id: search.job }
    : search.cron
      ? { kind: "cron", name: search.cron }
      : undefined;

  const openJob = useCallback(
    (row: ScheduledJobDoc) => setSearch({ job: row._id, cron: undefined }),
    [setSearch],
  );
  const openCron = useCallback(
    (row: CronJobDoc) =>
      setSearch({ cron: row.name ?? row._id, job: undefined }),
    [setSearch],
  );
  const closeSheet = useCallback(
    () => setSearch({ job: undefined, cron: undefined }),
    [setSearch],
  );

  const menuItems = useCallback(
    (state: MenuState): RowMenuItem[] => {
      if (state.kind === "job") {
        const row = state.row;
        const items: RowMenuItem[] = [
          { id: "open", label: "Open job", onSelect: () => void openJob(row) },
          {
            id: "run",
            label: "Run now",
            onSelect: () => void actions.runJob(row),
          },
        ];
        if ((row.status ?? "").toLowerCase() === "pending") {
          items.push({
            id: "cancel",
            label: "Cancel job",
            danger: true,
            onSelect: () => void actions.cancelJob(row),
          });
        }
        return items;
      }
      const row = state.row;
      return [
        { id: "open", label: "Open cron", onSelect: () => void openCron(row) },
        {
          id: "run",
          label: "Run now",
          onSelect: () => void actions.runCron(row),
        },
        {
          id: "delete",
          label: "Delete cron",
          danger: true,
          onSelect: () => setConfirmCron(row),
        },
      ];
    },
    [actions, openCron, openJob],
  );

  const deleteConfirmed = useCallback(async () => {
    if (!confirmCron) return;
    const ok = await actions.deleteCron(confirmCron);
    if (ok) {
      setConfirmCron(null);
      if (search.cron === (confirmCron.name ?? confirmCron._id)) closeSheet();
    }
  }, [actions, closeSheet, confirmCron, search.cron]);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-schedules"
    >
      <PageHeader
        title="Schedules"
        subtitle="Invocations for this tenant: one-shot scheduled jobs and recurring cron entries."
      />

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
        {section === "scheduled" ? (
          <ScheduledTable
            jobs={scheduled}
            tenant={activeTenant}
            onActivate={openJob}
            onMenu={(row, anchor) => setMenu({ ...anchor, kind: "job", row })}
          />
        ) : (
          <CronTable
            jobs={cron}
            tenant={activeTenant}
            onActivate={openCron}
            onMenu={(row, anchor) => setMenu({ ...anchor, kind: "cron", row })}
          />
        )}
      </div>

      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={
            menu.kind === "job"
              ? `Actions for ${menu.row.functionPath ?? menu.row._id}`
              : `Actions for ${menu.row.name ?? menu.row._id}`
          }
          items={menuItems(menu)}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="schedules-row-menu"
        />
      ) : null}

      <ScheduleSheet
        target={target}
        jobs={scheduled}
        crons={cron}
        actions={actions}
        onClose={closeSheet}
        onDeleteCron={setConfirmCron}
      />

      <ConfirmDialog
        open={confirmCron !== null}
        title={
          confirmCron
            ? `Delete cron "${confirmCron.name ?? confirmCron._id}"?`
            : "Delete cron?"
        }
        description="The scheduler stops running it and forgets its mutation. Declare it again to bring it back."
        confirmLabel="Delete cron"
        danger
        busy={
          confirmCron !== null &&
          actions.pending[confirmCron.name ?? confirmCron._id] === "delete"
        }
        onConfirm={() => void deleteConfirmed()}
        onCancel={() => setConfirmCron(null)}
        testid="schedules-delete-cron"
      />
    </section>
  );
}

const jobCol = dataColumns<ScheduledJobDoc>();

function ActionsCell({
  label,
  testid,
  onOpen,
}: {
  label: string;
  testid: string;
  onOpen: (anchor: RowAnchor) => void;
}) {
  return (
    <span className="flex justify-end">
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label={`Actions for ${label}`}
        data-testid={testid}
        onClick={(event) => {
          const rect = event.currentTarget.getBoundingClientRect();
          onOpen({
            x: rect.left,
            y: rect.bottom + 2,
            element: event.currentTarget,
          });
        }}
      >
        <Ellipsis />
      </Button>
    </span>
  );
}

function timeCell(at: number | undefined, fallback = "—") {
  return typeof at === "number" ? (
    <RelativeTime epochMs={at} />
  ) : (
    <span className="tabular text-text-3">{fallback}</span>
  );
}

function ScheduledTable({
  jobs,
  tenant,
  onActivate,
  onMenu,
}: {
  jobs: ScheduledJobDoc[] | undefined;
  tenant: string | null;
  onActivate: (row: ScheduledJobDoc) => void;
  onMenu: (row: ScheduledJobDoc, anchor: RowAnchor) => void;
}) {
  const serverUrl = useServerUrl();
  const columns = useMemo(
    () => [
      jobCol.accessor("functionPath", {
        header: "Function",
        size: 200,
        cell: (ctx) => (
          <span
            className="block truncate font-mono text-xs text-text-1"
            title={ctx.getValue() ?? ctx.row.original._id}
          >
            {ctx.getValue() ?? shortId(ctx.row.original._id, 12)}
          </span>
        ),
      }),
      jobCol.accessor("status", {
        header: "Status",
        size: 100,
        cell: (ctx) => <StatePill state={ctx.getValue()} />,
      }),
      jobCol.accessor("scheduledTime", {
        header: "Scheduled",
        size: 104,
        cell: (ctx) => timeCell(ctx.getValue()),
      }),
      jobCol.accessor((row) => row.result?.finishedAt, {
        id: "finishedAt",
        header: "Finished",
        size: 104,
        cell: (ctx) => timeCell(ctx.getValue()),
      }),
      jobCol.accessor((row) => row.result?.outcome, {
        id: "outcome",
        header: "Outcome",
        size: 150,
        cell: (ctx) => {
          const row = ctx.row.original;
          const outcome = ctx.getValue();
          if (!outcome) return <span className="tabular text-text-3">—</span>;
          return (
            <span
              className="block truncate font-mono text-xs text-text-1"
              title={row.result?.error ?? outcome}
            >
              {row.result?.error ? `${outcome}: ${row.result.error}` : outcome}
            </span>
          );
        },
      }),
      jobCol.display({
        id: "actions",
        header: () => <span className="sr-only">Actions</span>,
        size: 40,
        enableSorting: false,
        cell: (ctx) => (
          <ActionsCell
            label={ctx.row.original.functionPath ?? ctx.row.original._id}
            testid={`schedules-scheduled-actions-${ctx.row.original._id}`}
            onOpen={(anchor) => onMenu(ctx.row.original, anchor)}
          />
        ),
      }),
    ],
    [onMenu],
  );

  if (jobs !== undefined && jobs.length === 0) {
    return (
      <EmptyState
        title="No scheduled jobs"
        body="A scheduled job is one mutation the server runs later. Enqueue one from a function with ctx.scheduler.runAfter, or through the API; it appears here until it runs."
        snippet={scheduleJobCommand({ serverUrl, tenant, table: null })}
        testid="schedules-scheduled-empty"
      />
    );
  }
  return (
    <DataTable
      columns={columns}
      data={jobs ?? []}
      getRowId={(row) => row._id}
      ariaLabel="Scheduled jobs"
      loading={jobs === undefined}
      onRowActivate={onActivate}
      onRowContextMenu={onMenu}
      rowTestid={(row) => `schedules-scheduled-${row._id}`}
      testid="schedules-scheduled-table"
      className="h-full"
    />
  );
}

const cronCol = dataColumns<CronJobDoc>();

function CronTable({
  jobs,
  tenant,
  onActivate,
  onMenu,
}: {
  jobs: CronJobDoc[] | undefined;
  tenant: string | null;
  onActivate: (row: CronJobDoc) => void;
  onMenu: (row: CronJobDoc, anchor: RowAnchor) => void;
}) {
  const serverUrl = useServerUrl();
  const columns = useMemo(
    () => [
      cronCol.accessor("name", {
        header: "Name",
        size: 120,
        cell: (ctx) => (
          <span className="block truncate font-mono text-xs text-text-1">
            {ctx.getValue() ?? shortId(ctx.row.original._id, 12)}
          </span>
        ),
      }),
      cronCol.accessor("functionPath", {
        header: "Function",
        size: 170,
        cell: (ctx) => (
          <span className="block truncate font-mono text-xs text-text-1">
            {ctx.getValue() ?? "—"}
          </span>
        ),
      }),
      cronCol.accessor("schedule", {
        header: "Schedule",
        size: 96,
        cell: (ctx) => (
          <span
            className="font-mono text-xs text-text-1"
            title={ctx.getValue()}
          >
            {formatSchedule(ctx.getValue())}
          </span>
        ),
      }),
      cronCol.accessor("status", {
        header: "Status",
        size: 96,
        cell: (ctx) => <StatePill state={ctx.getValue()} />,
      }),
      cronCol.accessor("nextRunAt", {
        header: "Next run",
        size: 104,
        cell: (ctx) => timeCell(ctx.getValue()),
      }),
      cronCol.accessor("lastRunAt", {
        header: "Last run",
        size: 104,
        cell: (ctx) => timeCell(ctx.getValue(), "never"),
      }),
      cronCol.display({
        id: "actions",
        header: () => <span className="sr-only">Actions</span>,
        size: 40,
        enableSorting: false,
        cell: (ctx) => (
          <ActionsCell
            label={ctx.row.original.name ?? ctx.row.original._id}
            testid={`schedules-cron-actions-${ctx.row.original.name ?? ctx.row.original._id}`}
            onOpen={(anchor) => onMenu(ctx.row.original, anchor)}
          />
        ),
      }),
    ],
    [onMenu],
  );

  if (jobs !== undefined && jobs.length === 0) {
    return (
      <EmptyState
        title="No cron jobs"
        body="A cron job runs one mutation on a schedule. Register one through the API and it appears here with its schedule and next run."
        snippet={createCronCommand({ serverUrl, tenant, table: null })}
        testid="schedules-cron-empty"
      />
    );
  }
  return (
    <DataTable
      columns={columns}
      data={jobs ?? []}
      getRowId={(row) => row.name ?? row._id}
      ariaLabel="Cron jobs"
      loading={jobs === undefined}
      onRowActivate={onActivate}
      onRowContextMenu={onMenu}
      rowTestid={(row) => `schedules-cron-${row.name ?? row._id}`}
      testid="schedules-cron-table"
      className="h-full"
    />
  );
}
