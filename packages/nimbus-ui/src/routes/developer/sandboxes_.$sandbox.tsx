import { createFileRoute, useSearch } from "@tanstack/react-router";
import { Box, Square } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { Breadcrumb } from "../../components/breadcrumb";
import { CopyChip } from "../../components/copy-chip";
import { EmptyState } from "../../components/empty-state";
import { LoadingState } from "../../components/loading-state";
import { PageTabs } from "../../components/page-tabs";
import { CategoryPill, StatePill } from "../../components/pill";
import { RelativeTime } from "../../components/time";
import { Button } from "../../components/ui/button";
import { shortId } from "../../lib/format";
import type {
  RedactedValues,
  ResourceCondition,
  SandboxResource,
} from "../../lib/types/sandbox";
import { sandboxCanStop, sandboxDisplayName } from "../../lib/types/sandbox";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
import { useUiStore } from "../../store/ui-store";
import { StopSandboxDialog } from "./sandboxes";
import { useSandboxActions } from "./sandboxes/-sandbox-actions";
import { SandboxConsole } from "./sandboxes/-sandbox-console";
import { useSandbox, useSandboxList } from "./sandboxes/-sandbox-read";
import { SandboxSubPanel } from "./sandboxes/-sandbox-sub-panel";

export type DetailTab = "overview" | "console" | "spec";

export const TABS: ReadonlyArray<{ id: DetailTab; label: string }> = [
  { id: "overview", label: "Overview" },
  { id: "console", label: "Console" },
  { id: "spec", label: "Spec" },
];

type DetailSearch = {
  tab?: DetailTab;
};

export const Route = createFileRoute("/developer/sandboxes_/$sandbox")({
  validateSearch: (search: Record<string, unknown>): DetailSearch => ({
    tab: isTab(search.tab) ? search.tab : undefined,
  }),
  component: SandboxDetailPage,
});

export function isTab(value: unknown): value is DetailTab {
  return value === "overview" || value === "console" || value === "spec";
}

function SandboxDetailPage() {
  const { sandbox: sandboxId } = Route.useParams();
  const activeTenant = useUiStore((s) => s.activeTenant);
  if (activeTenant === null) {
    return (
      <section
        className="flex h-full flex-col overflow-hidden"
        data-testid="page-sandbox-detail"
      >
        <EmptyState
          icon={Box}
          title="Choose a tenant"
          body="A sandbox belongs to a tenant. Pick one in the sidebar to open this sandbox."
          cta={{ label: "Back to Sandboxes", to: "/developer/sandboxes" }}
          testid="sandbox-no-tenant"
        />
      </section>
    );
  }
  return <TenantSandboxDetail tenant={activeTenant} sandboxId={sandboxId} />;
}

function TenantSandboxDetail({
  tenant,
  sandboxId,
}: {
  tenant: string;
  sandboxId: string;
}) {
  const search = useSearch({ from: "/developer/sandboxes_/$sandbox" });
  const tab: DetailTab = search.tab ?? "overview";
  const [revision, setRevision] = useState(0);
  const bump = useCallback(() => setRevision((n) => n + 1), []);
  const read = useSandbox(tenant, sandboxId, revision);
  const list = useSandboxList(tenant, revision);
  const { pending, errors, stop } = useSandboxActions(bump);
  const [stopping, setStopping] = useState<SandboxResource | null>(null);

  const listRows =
    list.kind === "ok" && list.value.kind === "list"
      ? list.value.items.length
      : 0;
  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Sandboxes",
      search: { placeholder: "Filter sandboxes", rows: listRows },
      children: (
        <SandboxSubPanel
          read={list}
          activeId={sandboxId}
          activeTenant={tenant}
        />
      ),
    }),
    [list, listRows, sandboxId, tenant],
  );
  useContributeSubPanel(spec);

  if (read.kind === "loading") {
    return (
      <section
        className="flex h-full flex-col overflow-hidden"
        data-testid="page-sandbox-detail"
      >
        <LoadingState label="Reading sandbox…" testid="sandbox-loading" />
      </section>
    );
  }
  if (read.kind !== "ok" || read.value.kind === "missing") {
    const message =
      read.kind === "ok" && read.value.kind === "missing"
        ? read.value.message
        : read.kind === "error"
          ? read.message
          : "The server could not be reached.";
    return (
      <section
        className="flex h-full flex-col overflow-hidden"
        data-testid="page-sandbox-detail"
      >
        <EmptyState
          icon={Box}
          title="Sandbox not found"
          body={
            <>
              No sandbox <code>{sandboxId}</code> in tenant {tenant}. It may
              have stopped and been removed, or this server has no sandbox
              routes. The server said: {message}
            </>
          }
          cta={{ label: "Back to Sandboxes", to: "/developer/sandboxes" }}
          testid="sandbox-not-found"
        />
      </section>
    );
  }

  const sandbox = read.value.sandbox;
  const shownState = pending[sandboxId]
    ? "stopping"
    : sandbox.status.lifecycleState;
  const displayName = sandboxDisplayName(sandbox);

  return (
    <section
      className="flex h-full flex-col overflow-hidden"
      data-testid="page-sandbox-detail"
    >
      <div className="flex shrink-0 flex-col gap-3 border-b border-border-2 px-6 pb-3 pt-4">
        <Breadcrumb
          segments={[
            { label: "Sandboxes", href: "/developer/sandboxes" },
            { label: displayName, active: true },
          ]}
        />
        <header className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex flex-wrap items-baseline gap-3">
            <h1
              className="font-mono text-text-1"
              style={{ fontSize: "var(--text-lg)" }}
            >
              {displayName}
            </h1>
            <CategoryPill value={sandbox.spec.profile} />
            <CategoryPill value={sandbox.status.backend} />
            <StatePill state={shownState} data-testid="sandbox-detail-state" />
            {displayName !== sandboxId ? (
              <CopyChip
                label="sandbox id"
                value={sandboxId}
                testid="sandbox-detail-id"
              >
                {shortId(sandboxId, 16)}
              </CopyChip>
            ) : null}
          </div>
          <div className="flex flex-col items-end gap-1">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!sandboxCanStop(shownState)}
              onClick={() => setStopping(sandbox)}
              data-testid="sandbox-detail-stop"
            >
              <Square className="size-3.5" aria-hidden /> Stop
            </Button>
            {errors[sandboxId] ? (
              <span
                className="text-xs text-error"
                data-testid="sandbox-detail-error"
              >
                {errors[sandboxId]}
              </span>
            ) : null}
          </div>
        </header>
        <PageTabs
          label="Sandbox detail sections"
          tabs={TABS}
          active={tab}
          testid="sandbox-detail-tabs"
          itemTestid="sandbox-detail-tab"
        />
      </div>

      <div className="min-h-0 flex-1 overflow-hidden">
        {tab === "console" ? (
          <SandboxConsole
            tenant={tenant}
            sandboxId={sandboxId}
            lifecycleState={sandbox.status.lifecycleState}
            testid="sandbox-console"
          />
        ) : tab === "spec" ? (
          <SpecTab sandbox={sandbox} />
        ) : (
          <OverviewTab sandbox={sandbox} shownState={shownState} />
        )}
      </div>
      <StopSandboxDialog
        sandbox={stopping}
        onConfirm={() => {
          if (stopping) void stop(stopping);
          setStopping(null);
        }}
        onCancel={() => setStopping(null)}
      />
    </section>
  );
}

function OverviewTab({
  sandbox,
  shownState,
}: {
  sandbox: SandboxResource;
  shownState: string;
}) {
  const { metadata, status } = sandbox;
  const labels = Object.entries(metadata.labels ?? {});
  return (
    <div
      className="flex h-full flex-col gap-5 overflow-auto px-6 py-4 text-sm text-text-1"
      data-testid="sandbox-tab-overview"
    >
      <div className="flex flex-col gap-2">
        <Stat label="Id" value={metadata.id} />
        <Stat label="Tenant" value={metadata.tenantId} />
        <Stat label="Profile" value={sandbox.spec.profile} />
        <Stat label="Backend" value={status.backend} />
        <Stat label="State" value={<StatePill state={shownState} />} />
        <Stat label="Readiness" value={status.readiness} />
        <Stat label="Health" value={status.health} />
        <Stat label="Generation" value={String(metadata.generation)} />
        <Stat label="Created" value={<Timestamp iso={metadata.createdAt} />} />
        <Stat label="Updated" value={<Timestamp iso={metadata.updatedAt} />} />
        <Stat
          label="Labels"
          value={
            labels.length === 0
              ? "—"
              : labels.map(([key, value]) => `${key}=${value}`).join(" ")
          }
        />
      </div>

      <Panel title="Endpoints" testid="sandbox-endpoints">
        {status.endpoints.length === 0 ? (
          <p className="text-xs text-text-3">
            No endpoints published. A sandbox that listens on a port shows it
            here once the runtime reports it.
          </p>
        ) : (
          <ul className="flex flex-col gap-1">
            {status.endpoints.map((endpoint) => (
              <li
                key={`${endpoint.name}:${endpoint.host}:${endpoint.port}`}
                className="flex items-baseline gap-3 font-mono text-xs"
              >
                <span className="w-32 text-text-3">{endpoint.name}</span>
                <span className="text-text-1">
                  {endpoint.host}:{endpoint.port}
                </span>
                <span className="text-text-3">{endpoint.protocol}</span>
              </li>
            ))}
          </ul>
        )}
      </Panel>

      <Panel title="Conditions" testid="sandbox-conditions">
        <p className="text-xs text-text-3">
          The lifecycle as the manager observed it. The manager records no event
          stream for a sandbox; these conditions and the Console tab are its
          history.
        </p>
        {status.conditions.length === 0 ? (
          <p className="text-xs text-text-3">No conditions reported yet.</p>
        ) : (
          <ul className="flex flex-col gap-1">
            {status.conditions.map((condition) => (
              <ConditionRow
                key={`${condition.type}:${condition.lastTransitionTime}`}
                condition={condition}
              />
            ))}
          </ul>
        )}
      </Panel>
    </div>
  );
}

function ConditionRow({ condition }: { condition: ResourceCondition }) {
  return (
    <li
      className="flex flex-wrap items-baseline gap-3 font-mono text-xs"
      data-testid={`sandbox-condition-${condition.type}`}
    >
      <span className="w-32 text-text-3">{condition.type}</span>
      <span className="text-text-1">{condition.status}</span>
      {condition.reason ? (
        <span className="text-text-3">{condition.reason}</span>
      ) : null}
      {condition.message ? (
        <span className="text-text-3">{condition.message}</span>
      ) : null}
      <span className="ml-auto text-text-3">
        <Timestamp iso={condition.lastTransitionTime} />
      </span>
    </li>
  );
}

function SpecTab({ sandbox }: { sandbox: SandboxResource }) {
  const { owner, root, process, backend } = sandbox.spec.sandbox;
  return (
    <div
      className="flex h-full flex-col gap-5 overflow-auto px-6 py-4 text-sm text-text-1"
      data-testid="sandbox-tab-spec"
    >
      <Panel title="Owner" testid="sandbox-spec-owner">
        <Stat label="Kind" value={owner.kind} />
        {owner.kind === "standalone" ? (
          <Stat label="Display name" value={owner.displayName ?? "—"} />
        ) : (
          <Stat label="Service" value={owner.serviceName} />
        )}
      </Panel>
      <Panel title="Root" testid="sandbox-spec-root">
        <Stat label="Backend" value={backend} />
        <Stat label="Root kind" value={root.kind} />
        <Stat
          label="Image"
          value={
            root.kind === "oci_image"
              ? root.source.reference
              : `redacted: ${root.reason}`
          }
        />
      </Panel>
      <Panel title="Process" testid="sandbox-spec-process">
        <Stat label="Command" value={<Redacted values={process.argv} />} />
        {process.entrypoint ? (
          <Stat
            label="Entrypoint"
            value={<Redacted values={process.entrypoint} />}
          />
        ) : null}
        <Stat label="Working dir" value={process.cwd || "—"} />
        <Stat label="User" value={process.user ?? "—"} />
        <Stat
          label="Environment"
          value={<Redacted values={process.environment} />}
        />
        <Stat label="Terminal" value={process.terminal ? "yes" : "no"} />
      </Panel>
    </div>
  );
}

// The spec route never returns argument or environment values, only how
// many there are: a sandbox spec can carry secrets.
function Redacted({ values }: { values: RedactedValues }) {
  const count = values.valueCount;
  return (
    <span className="text-text-3">
      {count} value{count === 1 ? "" : "s"}
      {values.redacted ? ", redacted" : ""}
    </span>
  );
}

function Timestamp({ iso }: { iso: string }) {
  const at = Date.parse(iso);
  return Number.isFinite(at) ? (
    <RelativeTime epochMs={at} />
  ) : (
    <span className="tabular text-text-3">—</span>
  );
}

function Panel({
  title,
  testid,
  children,
}: {
  title: string;
  testid: string;
  children: React.ReactNode;
}) {
  return (
    <section
      className="flex flex-col gap-2 rounded-md border border-border-2 bg-bg-panel px-4 py-3"
      data-testid={testid}
    >
      <h2 className="text-xs font-medium text-text-3">{title}</h2>
      {children}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="w-32 shrink-0 text-xs font-medium text-text-3">
        {label}
      </span>
      <span className="min-w-0 break-all font-mono text-xs text-text-1">
        {value}
      </span>
    </div>
  );
}
