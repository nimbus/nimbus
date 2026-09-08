import type { Meta, StoryObj } from "@storybook/react";
import { ChevronsLeft, ChevronsRight, Cpu, FunctionSquare } from "lucide-react";
import { useState } from "react";

import { cn } from "@/lib/utils";
import {
  type DynamicSubPanelSpec,
  type StaticSubPanelSpec,
  SUB_PANEL_DEFAULT_WIDTH,
  SUB_PANEL_RAIL_WIDTH,
  type SubPanelItem,
  type SubPanelRailItem,
  type SubPanelSpec,
  showsSubPanelSearch,
} from "../shell/sub-panel";

const meta: Meta = {
  title: "Shell/SubPanel",
};

export default meta;

type Story = StoryObj;

// The shell's SubPanelLayout needs the router and a resizable group; the
// story shows the panel's visual contract at a fixed width instead, with the
// same header, search rule, rows, and rail as the shell renders.
function FakeSubPanelHost({
  spec,
  initialSearch = "",
  activeId,
  width = SUB_PANEL_DEFAULT_WIDTH,
}: {
  spec: SubPanelSpec;
  initialSearch?: string;
  activeId?: string;
  width?: number;
}) {
  const [search, setSearch] = useState(initialSearch);
  return (
    <aside
      aria-label={spec.title}
      data-testid="sub-panel"
      data-kind={spec.kind}
      data-collapsed="false"
      style={{ width }}
      className="flex h-[420px] shrink-0 flex-col border-r border-border-2 bg-bg-panel"
    >
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-border-2 px-3">
        <span className="truncate text-xs font-medium text-text-3">
          {spec.title}
        </span>
        <button
          type="button"
          aria-label="Collapse sub-panel"
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-sm text-text-3 transition-colors hover:bg-bg-hover hover:text-text-1"
        >
          <ChevronsLeft size={14} aria-hidden />
        </button>
      </header>
      {spec.kind === "dynamic" && spec.search && showsSubPanelSearch(spec) ? (
        <div className="shrink-0 border-b border-border-2 px-3 py-2">
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={spec.search.placeholder}
            data-testid="sub-panel-search"
            className="h-7 w-full rounded-sm border border-border-2 bg-bg-canvas px-2 text-xs text-text-1 placeholder:text-text-3"
          />
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-auto">
        {spec.kind === "static" ? (
          <FakeStaticList items={spec.items} activeId={activeId} />
        ) : (
          spec.children
        )}
      </div>
    </aside>
  );
}

function FakeStaticList({
  items,
  activeId,
}: {
  items: ReadonlyArray<SubPanelItem<string>>;
  activeId?: string;
}) {
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {items.map((item) => {
        const active = item.id === activeId;
        const row = cn(
          "flex h-8 items-center gap-2 rounded-sm border-l-2 border-transparent px-2 text-sm no-underline",
          item.disabled
            ? "text-text-3"
            : active
              ? "bg-bg-hover text-text-1"
              : "text-text-3 hover:bg-bg-hover hover:text-text-1",
        );
        if (item.disabled) {
          return (
            <li key={item.id}>
              <span
                aria-disabled="true"
                className={cn(row, "cursor-not-allowed")}
              >
                <span className="flex-1 truncate">{item.label}</span>
                <span
                  aria-hidden
                  className="inline-flex items-center rounded-xs border border-border-2 bg-bg-raised px-1.5 py-0.5 text-xs font-medium leading-none text-text-3"
                >
                  coming soon
                </span>
              </span>
            </li>
          );
        }
        return (
          <li key={item.id}>
            <a
              href={item.to}
              data-testid={`sub-panel-item-${item.id}`}
              data-active={active ? "true" : "false"}
              className={row}
              style={active ? { borderLeftColor: "var(--accent)" } : undefined}
            >
              <span className="flex-1 truncate">{item.label}</span>
              {typeof item.count === "number" ? (
                <span className="tabular font-mono text-xs text-text-3">
                  {item.count}
                </span>
              ) : null}
            </a>
          </li>
        );
      })}
    </ul>
  );
}

// The collapsed rail: the expand toggle and one icon per sub-view.
function FakeSubPanelRail({
  title,
  railItems,
}: {
  title: string;
  railItems: ReadonlyArray<SubPanelRailItem>;
}) {
  return (
    <aside
      aria-label={title}
      data-testid="sub-panel"
      data-collapsed="true"
      style={{ width: SUB_PANEL_RAIL_WIDTH }}
      className="flex h-[420px] shrink-0 flex-col gap-1 border-r border-border-2 bg-bg-panel py-2"
    >
      <button
        type="button"
        aria-label="Expand sub-panel"
        className="flex h-8 w-full items-center justify-center rounded-sm text-text-3 transition-colors hover:bg-bg-hover hover:text-text-1"
      >
        <ChevronsRight size={14} aria-hidden />
      </button>
      {railItems.map((item) => {
        const Icon = item.icon;
        return (
          <button
            key={item.id}
            type="button"
            aria-label={item.label}
            aria-current={item.active ? "page" : undefined}
            className={cn(
              "flex h-8 w-full items-center justify-center border-l-2 border-transparent text-text-3 transition-colors hover:bg-bg-hover hover:text-text-1",
              item.active && "bg-bg-hover text-text-1",
            )}
            style={
              item.active ? { borderLeftColor: "var(--accent)" } : undefined
            }
          >
            <Icon size={14} aria-hidden />
          </button>
        );
      })}
    </aside>
  );
}

const STATIC_SPEC: StaticSubPanelSpec = {
  kind: "static",
  title: "Storage",
  items: [
    { id: "tenants", label: "Tenants", to: "/operator/tenants", count: 4 },
    { id: "tables", label: "Tables", to: "/operator/tables", count: 17 },
    { id: "documents", label: "Documents", to: "/operator/documents" },
    {
      id: "indexes",
      label: "Indexes",
      to: "/operator/indexes",
      disabled: true,
    },
  ],
};

const SERVICES = [
  { id: "api", label: "api", state: "running" },
  { id: "web", label: "web", state: "running" },
  { id: "worker", label: "worker", state: "stopped" },
];

function ServiceRows({
  rows,
}: {
  rows: ReadonlyArray<{ id: string; label: string; state: string }>;
}) {
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {rows.map((svc) => (
        <li key={svc.id}>
          <a
            href={`/developer/services/${svc.id}`}
            data-testid={`sub-panel-item-dev-service-${svc.label}`}
            className="flex h-8 items-center gap-2 rounded-sm px-2 text-sm text-text-3 no-underline hover:bg-bg-hover hover:text-text-1"
          >
            <span className="flex-1 truncate font-mono text-xs">
              {svc.label}
            </span>
            <span className="tabular text-xs font-medium text-text-3">
              {svc.state}
            </span>
          </a>
        </li>
      ))}
    </ul>
  );
}

const DYNAMIC_SPEC: DynamicSubPanelSpec = {
  kind: "dynamic",
  title: "Services",
  search: { placeholder: "Filter services", rows: SERVICES.length },
  children: <ServiceRows rows={SERVICES} />,
};

const MANY_SERVICES = Array.from({ length: 24 }, (_, i) => ({
  id: `svc-${i}`,
  label: `service-${String(i).padStart(2, "0")}`,
  state: i % 5 === 0 ? "stopped" : "running",
}));

const LONG_DYNAMIC_SPEC: DynamicSubPanelSpec = {
  kind: "dynamic",
  title: "Services",
  search: { placeholder: "Filter services", rows: MANY_SERVICES.length },
  children: <ServiceRows rows={MANY_SERVICES} />,
};

export const StaticList: Story = {
  render: () => <FakeSubPanelHost spec={STATIC_SPEC} activeId="tables" />,
};

export const StaticListNoneActive: Story = {
  render: () => <FakeSubPanelHost spec={STATIC_SPEC} />,
};

// Three rows: the list scans faster than it filters, so no search field.
export const DynamicShortList: Story = {
  render: () => <FakeSubPanelHost spec={DYNAMIC_SPEC} />,
};

// Past twenty rows the search field appears under the header.
export const DynamicWithSearch: Story = {
  render: () => <FakeSubPanelHost spec={LONG_DYNAMIC_SPEC} />,
};

export const DynamicEmpty: Story = {
  render: () => (
    <FakeSubPanelHost
      spec={{
        kind: "dynamic",
        title: "Services",
        search: { placeholder: "Filter services", rows: 0 },
        children: (
          <div className="px-3 py-6 text-xs text-text-3">
            <p>No services declared.</p>
            <p className="mt-2">
              Author a compose.yaml and run{" "}
              <code className="font-mono">nimbus compose up</code>.
            </p>
          </div>
        ),
      }}
    />
  ),
};

export const Widths: Story = {
  render: () => (
    <div className="flex gap-4">
      <FakeSubPanelHost spec={STATIC_SPEC} activeId="tables" width={180} />
      <FakeSubPanelHost spec={STATIC_SPEC} activeId="tables" width={240} />
      <FakeSubPanelHost spec={STATIC_SPEC} activeId="tables" width={400} />
    </div>
  ),
};

export const CollapsedRail: Story = {
  render: () => (
    <FakeSubPanelRail
      title="Compute"
      railItems={[
        {
          id: "functions",
          label: "Functions",
          icon: FunctionSquare,
          active: true,
          onSelect: () => {},
        },
        {
          id: "sandboxes",
          label: "Sandboxes",
          icon: Cpu,
          active: false,
          onSelect: () => {},
        },
      ]}
    />
  ),
};
