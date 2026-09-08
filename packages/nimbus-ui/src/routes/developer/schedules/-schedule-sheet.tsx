import { CategoryPill, StatePill } from "../../../components/pill";
import { RelativeTime } from "../../../components/time";
import { Button } from "../../../components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "../../../components/ui/sheet";
import { shortId } from "../../../lib/format";
import { jobIdFromDocumentId } from "./-job-ids";
import {
  type CronJobDoc,
  formatSchedule,
  type ScheduledJobDoc,
} from "./-types";
import type { ScheduleActions } from "./-use-schedule-actions";

const TESTID = "schedules-sheet";

export type SheetTarget =
  | { kind: "job"; id: string }
  | { kind: "cron"; name: string };

// ScheduleSheet is the right-side detail for one scheduled job or one cron:
// the facts the row cannot fit, the mutation the scheduler will apply, the
// result of the last run, and the same actions the row menu offers.
export function ScheduleSheet({
  target,
  jobs,
  crons,
  actions,
  onClose,
  onDeleteCron,
}: {
  target: SheetTarget | undefined;
  jobs: readonly ScheduledJobDoc[] | undefined;
  crons: readonly CronJobDoc[] | undefined;
  actions: ScheduleActions;
  onClose: () => void;
  onDeleteCron: (cron: CronJobDoc) => void;
}) {
  const job =
    target?.kind === "job" ? jobs?.find((j) => j._id === target.id) : undefined;
  const cron =
    target?.kind === "cron"
      ? crons?.find((c) => c.name === target.name)
      : undefined;
  const loading =
    target?.kind === "job" ? jobs === undefined : crons === undefined;
  return (
    <Sheet
      open={target !== undefined}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <SheetContent
        className="w-full sm:max-w-md"
        data-testid={TESTID}
        aria-label={
          job
            ? `Scheduled job ${job.functionPath ?? job._id}`
            : cron
              ? `Cron ${cron.name ?? cron._id}`
              : "Schedule"
        }
      >
        {job ? (
          <JobBody job={job} actions={actions} />
        ) : cron ? (
          <CronBody cron={cron} actions={actions} onDelete={onDeleteCron} />
        ) : target ? (
          <Missing loading={loading} />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function JobBody({
  job,
  actions,
}: {
  job: ScheduledJobDoc;
  actions: ScheduleActions;
}) {
  const inFlight = actions.pending[job._id];
  const cancellable = (job.status ?? "").toLowerCase() === "pending";
  return (
    <>
      <SheetHeader className="pr-12">
        <SheetTitle className="flex min-w-0 items-center gap-2">
          <span
            className="min-w-0 truncate font-mono text-sm text-text-1"
            title={job.functionPath ?? job._id}
          >
            {job.functionPath ?? shortId(job._id, 12)}
          </span>
          <StatePill state={job.status} data-testid={`${TESTID}-status`} />
        </SheetTitle>
        <SheetDescription className="flex items-center gap-2">
          <CategoryPill value="scheduled" />
          <span className="font-mono text-xs text-text-3">
            {jobIdFromDocumentId(job._id) ?? shortId(job._id, 14)}
          </span>
        </SheetDescription>
      </SheetHeader>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto px-4">
        <Facts>
          <Fact label="Tenant">{job.tenantId ?? "—"}</Fact>
          <Fact label="Scheduled">
            {typeof job.scheduledTime === "number" ? (
              <RelativeTime epochMs={job.scheduledTime} />
            ) : (
              "—"
            )}
          </Fact>
          <Fact label="Finished">
            {typeof job.result?.finishedAt === "number" ? (
              <RelativeTime epochMs={job.result.finishedAt} />
            ) : (
              "—"
            )}
          </Fact>
          <Fact label="Outcome">{job.result?.outcome ?? "—"}</Fact>
        </Facts>
        <Block title="Mutation" testid={`${TESTID}-args`}>
          {job.args
            ? JSON.stringify(job.args, null, 2)
            : "No mutation recorded."}
        </Block>
        {job.result?.error ? (
          <Block title="Error" testid={`${TESTID}-error`} tone="error">
            {job.result.error}
          </Block>
        ) : null}
      </div>
      <SheetFooter className="flex-row justify-end">
        {cancellable ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={inFlight !== undefined}
            onClick={() => void actions.cancelJob(job)}
            data-testid={`${TESTID}-cancel`}
          >
            Cancel job
          </Button>
        ) : null}
        <Button
          type="button"
          size="sm"
          disabled={inFlight !== undefined}
          onClick={() => void actions.runJob(job)}
          data-testid={`${TESTID}-run`}
        >
          Run now
        </Button>
      </SheetFooter>
    </>
  );
}

function CronBody({
  cron,
  actions,
  onDelete,
}: {
  cron: CronJobDoc;
  actions: ScheduleActions;
  onDelete: (cron: CronJobDoc) => void;
}) {
  const key = cron.name ?? cron._id;
  const inFlight = actions.pending[key];
  return (
    <>
      <SheetHeader className="pr-12">
        <SheetTitle className="flex min-w-0 items-center gap-2">
          <span className="min-w-0 truncate font-mono text-sm text-text-1">
            {cron.name ?? shortId(cron._id, 12)}
          </span>
          <StatePill state={cron.status} data-testid={`${TESTID}-status`} />
        </SheetTitle>
        <SheetDescription className="flex items-center gap-2">
          <CategoryPill value="cron" />
          <span className="font-mono text-xs text-text-3">
            {formatSchedule(cron.schedule)}
          </span>
        </SheetDescription>
      </SheetHeader>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto px-4">
        <Facts>
          <Fact label="Tenant">{cron.tenantId ?? "—"}</Fact>
          <Fact label="Function">{cron.functionPath ?? "—"}</Fact>
          <Fact label="Schedule">{cron.schedule ?? "—"}</Fact>
          <Fact label="Next run">
            {typeof cron.nextRunAt === "number" ? (
              <RelativeTime epochMs={cron.nextRunAt} />
            ) : (
              "—"
            )}
          </Fact>
          <Fact label="Last run">
            {typeof cron.lastRunAt === "number" ? (
              <RelativeTime epochMs={cron.lastRunAt} />
            ) : (
              "never"
            )}
          </Fact>
        </Facts>
        <p className="text-xs text-text-3">
          Run now enqueues the mutation this cron applies as a one-shot job; the
          cron keeps its own next run.
        </p>
      </div>
      <SheetFooter className="flex-row justify-end">
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={inFlight !== undefined}
          onClick={() => onDelete(cron)}
          data-testid={`${TESTID}-delete`}
        >
          Delete cron
        </Button>
        <Button
          type="button"
          size="sm"
          disabled={inFlight !== undefined}
          onClick={() => void actions.runCron(cron)}
          data-testid={`${TESTID}-run`}
        >
          Run now
        </Button>
      </SheetFooter>
    </>
  );
}

function Missing({ loading }: { loading: boolean }) {
  return (
    <>
      <SheetHeader className="pr-12">
        <SheetTitle>{loading ? "Reading…" : "Not in this list"}</SheetTitle>
        <SheetDescription>
          {loading
            ? "The schedule list is still loading."
            : "The row the address names is not in the list any more. It may have completed and been pruned, or it belongs to another tenant."}
        </SheetDescription>
      </SheetHeader>
      <div className="flex-1" data-testid={`${TESTID}-missing`} />
    </>
  );
}

function Facts({ children }: { children: React.ReactNode }) {
  return (
    <dl className="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-1.5 text-xs">
      {children}
    </dl>
  );
}

function Fact({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <>
      <dt className="font-medium text-text-3">{label}</dt>
      <dd className="m-0 min-w-0 truncate font-mono text-text-1">{children}</dd>
    </>
  );
}

function Block({
  title,
  testid,
  tone,
  children,
}: {
  title: string;
  testid: string;
  tone?: "error";
  children: React.ReactNode;
}) {
  return (
    <section className="flex flex-col gap-1.5" data-testid={testid}>
      <h3 className="text-xs font-medium text-text-3">{title}</h3>
      <pre
        className={
          tone === "error"
            ? "m-0 overflow-auto rounded-md border border-error bg-error-tint p-3 font-mono text-xs whitespace-pre-wrap text-text-1"
            : "m-0 overflow-auto rounded-md border border-border-2 bg-bg-raised p-3 font-mono text-xs whitespace-pre-wrap text-text-1"
        }
      >
        {children}
      </pre>
    </section>
  );
}
