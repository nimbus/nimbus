import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { cn } from "../lib/cn";
import type {
  DynamicSubDrawerSpec,
  StaticSubDrawerSpec,
  SubDrawerItem,
  SubDrawerSpec,
} from "../shell/sub-drawer";

const meta: Meta = {
  title: "Shell/SubDrawer",
};

export default meta;

type Story = StoryObj;

function FakeSubDrawerHost({
  spec,
  initialSearch = "",
  activeId,
}: {
  spec: SubDrawerSpec;
  initialSearch?: string;
  activeId?: string;
}) {
  const [search, setSearch] = useState(initialSearch);
  return (
    <aside
      aria-label={spec.title}
      data-testid="sub-drawer"
      data-kind={spec.kind}
      className="flex h-[420px] w-64 shrink-0 flex-col border-r border-app bg-surface"
    >
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-app px-3">
        <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
          {spec.title}
        </span>
      </header>
      {spec.kind === "dynamic" && spec.search ? (
        <div className="border-b border-app px-3 py-2">
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={spec.search.placeholder}
            data-testid="sub-drawer-search"
            className="h-7 w-full rounded-md border border-app bg-app px-2 text-xs text-default placeholder:text-muted focus:outline-none focus:ring-1 focus:ring-[color:var(--color-brand)]"
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
  items: ReadonlyArray<SubDrawerItem<string>>;
  activeId?: string;
}) {
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {items.map((item) => {
        const active = item.id === activeId;
        return (
          <li key={item.id}>
            <a
              href={item.to}
              data-testid={`sub-drawer-item-${item.id}`}
              data-active={active ? "true" : "false"}
              className={cn(
                "flex h-8 items-center gap-2 rounded-md border-l-2 border-transparent px-2 text-sm no-underline",
                item.disabled
                  ? "pointer-events-none text-muted opacity-60"
                  : active
                    ? "bg-surface-2 text-default"
                    : "text-muted hover:bg-surface-2 hover:text-default",
              )}
              style={
                active ? { borderLeftColor: "var(--color-brand)" } : undefined
              }
            >
              <span className="flex-1 truncate">{item.label}</span>
              {typeof item.count === "number" ? (
                <span className="tabular font-mono text-xs text-muted">
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

const STATIC_SPEC: StaticSubDrawerSpec = {
  kind: "static",
  title: "Storage",
  items: [
    { id: "tenants", label: "Tenants", to: "/admin/tenants", count: 4 },
    { id: "tables", label: "Tables", to: "/admin/tables", count: 17 },
    { id: "documents", label: "Documents", to: "/admin/documents" },
    { id: "indexes", label: "Indexes", to: "/admin/indexes", disabled: true },
  ],
};

const DYNAMIC_SPEC: DynamicSubDrawerSpec = {
  kind: "dynamic",
  title: "Services",
  search: { placeholder: "Filter services" },
  children: (
    <ul className="flex flex-col gap-px px-2 py-2">
      {[
        { id: "api", label: "api", state: "running" },
        { id: "web", label: "web", state: "running" },
        { id: "worker", label: "worker", state: "stopped" },
      ].map((svc) => (
        <li key={svc.id}>
          <a
            href={`/app/services/${svc.id}`}
            data-testid={`sub-drawer-item-dev-service-${svc.label}`}
            className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-muted hover:bg-surface-2 hover:text-default no-underline"
          >
            <span className="flex-1 truncate font-mono text-xs">
              {svc.label}
            </span>
            <span className="tabular font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
              {svc.state}
            </span>
          </a>
        </li>
      ))}
    </ul>
  ),
};

export const StaticList: Story = {
  render: () => <FakeSubDrawerHost spec={STATIC_SPEC} activeId="tables" />,
};

export const StaticListNoneActive: Story = {
  render: () => <FakeSubDrawerHost spec={STATIC_SPEC} />,
};

export const DynamicWithSearch: Story = {
  render: () => <FakeSubDrawerHost spec={DYNAMIC_SPEC} />,
};

export const DynamicEmpty: Story = {
  render: () => (
    <FakeSubDrawerHost
      spec={{
        kind: "dynamic",
        title: "Services",
        search: { placeholder: "Filter services" },
        children: (
          <div className="px-3 py-6 text-xs text-muted">
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
