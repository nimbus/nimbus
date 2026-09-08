import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { cn } from "@/lib/utils";
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
      className="flex h-[420px] w-64 shrink-0 flex-col border-r border-border-2 bg-bg-panel"
    >
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-border-2 px-3">
        <span className="text-xs font-medium text-text-3">{spec.title}</span>
      </header>
      {spec.kind === "dynamic" && spec.search ? (
        <div className="border-b border-border-2 px-3 py-2">
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={spec.search.placeholder}
            data-testid="sub-drawer-search"
            // A story is where the next author looks for the pattern, so the
            // failing `--brand` ring must not be modelled here either: 2.24:1
            // in warm light on this field's own `bg-bg-canvas` ground, under SC
            // 1.4.11's 3:1 floor.
            className="h-7 w-full rounded-md border border-border-2 bg-bg-canvas px-2 text-xs text-text-1 placeholder:text-text-3"
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
                  ? "pointer-events-none text-text-3 opacity-60"
                  : active
                    ? "bg-bg-raised text-text-1"
                    : "text-text-3 hover:bg-bg-raised hover:text-text-1",
              )}
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

const STATIC_SPEC: StaticSubDrawerSpec = {
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
            href={`/developer/services/${svc.id}`}
            data-testid={`sub-drawer-item-dev-service-${svc.label}`}
            className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-text-3 hover:bg-bg-raised hover:text-text-1 no-underline"
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
