import { Link, useRouterState } from "@tanstack/react-router";
import { ChevronsLeft, ChevronsRight, type LucideIcon } from "lucide-react";
import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { cn } from "../lib/cn";
import { useUiStore } from "../store/ui-store";

// Background double-click toggles collapse; ignore double-clicks that land on an
// interactive element (links, buttons, inputs) so they keep their own behavior.
function isInteractiveTarget(target: EventTarget | null): boolean {
  return Boolean(
    (target as HTMLElement | null)?.closest("a,button,input,textarea,select"),
  );
}

export type SubDrawerItem<TId extends string = string> = {
  readonly id: TId;
  readonly label: string;
  readonly to: string;
  readonly search?: Record<string, unknown>;
  readonly description?: string;
  readonly disabled?: boolean;
  readonly count?: number | null;
};

// Items shown in the collapsed rail: each sub-view as an icon with the active
// one indicated. Clicking one switches sub-view without expanding the drawer.
export type SubDrawerRailItem = {
  readonly id: string;
  readonly label: string;
  readonly icon: LucideIcon;
  readonly active: boolean;
  readonly onSelect: () => void;
};

export type StaticSubDrawerSpec<TId extends string = string> = {
  readonly kind: "static";
  readonly title: string;
  readonly items: ReadonlyArray<SubDrawerItem<TId>>;
  readonly railItems?: ReadonlyArray<SubDrawerRailItem>;
};

export type DynamicSubDrawerSpec = {
  readonly kind: "dynamic";
  readonly title: string;
  readonly search?: { placeholder: string };
  readonly children: ReactNode;
  readonly railItems?: ReadonlyArray<SubDrawerRailItem>;
};

export type SubDrawerSpec<TId extends string = string> =
  | StaticSubDrawerSpec<TId>
  | DynamicSubDrawerSpec;

type SubDrawerContextValue = {
  spec: SubDrawerSpec | null;
  setSpec: (spec: SubDrawerSpec | null) => void;
  search: string;
  setSearch: (next: string) => void;
};

const SubDrawerContext = createContext<SubDrawerContextValue | null>(null);

export function SubDrawerProvider({ children }: { children: ReactNode }) {
  const [spec, setSpec] = useState<SubDrawerSpec | null>(null);
  const [search, setSearch] = useState<string>("");
  const value = useMemo(
    () => ({ spec, setSpec, search, setSearch }),
    [spec, search],
  );
  return (
    <SubDrawerContext.Provider value={value}>
      {children}
    </SubDrawerContext.Provider>
  );
}

export function useSubDrawerSearch(): string {
  const ctx = useContext(SubDrawerContext);
  if (!ctx) {
    throw new Error("useSubDrawerSearch must be used within SubDrawerProvider");
  }
  return ctx.search;
}

export function useContributeSubDrawer(spec: SubDrawerSpec | null) {
  const ctx = useContext(SubDrawerContext);
  if (!ctx) {
    throw new Error(
      "useContributeSubDrawer must be used within a SubDrawerProvider",
    );
  }
  const { setSpec, setSearch } = ctx;
  useEffect(() => {
    setSpec(spec);
    setSearch("");
    return () => {
      setSpec(null);
      setSearch("");
    };
  }, [spec, setSpec, setSearch]);
}

export function SubDrawer() {
  const ctx = useContext(SubDrawerContext);
  const open = useUiStore((s) => s.subDrawerOpen);
  const setSubDrawerOpen = useUiStore((s) => s.setSubDrawerOpen);
  const toggleSubDrawer = useUiStore((s) => s.toggleSubDrawer);
  const spec = ctx?.spec ?? null;
  const search = ctx?.search ?? "";
  const setSearch = ctx?.setSearch ?? (() => {});
  if (!spec) return null;

  // Collapsed: a thin rail with an expand toggle (mirrors the primary drawer).
  // The panel is never fully removed, so it is always reachable again.
  if (!open) {
    return (
      <aside
        aria-label={spec.title}
        data-testid="sub-drawer"
        data-kind={spec.kind}
        data-collapsed="true"
        onDoubleClick={(e) => {
          if (!isInteractiveTarget(e.target)) toggleSubDrawer();
        }}
        className="flex h-full w-8 shrink-0 flex-col gap-1 border-r border-app bg-surface py-2"
      >
        <button
          type="button"
          onClick={() => setSubDrawerOpen(true)}
          aria-label="Expand sub-drawer"
          title="Expand sub-drawer"
          data-testid="sub-drawer-toggle"
          className="flex h-7 w-full items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-default"
        >
          <ChevronsRight size={12} aria-hidden />
        </button>
        {spec.railItems?.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              type="button"
              onClick={item.onSelect}
              aria-label={item.label}
              aria-current={item.active ? "page" : undefined}
              title={item.label}
              data-testid={`sub-drawer-rail-item-${item.id}`}
              data-active={item.active ? "true" : "false"}
              className={cn(
                "flex h-8 w-full items-center justify-center border-l-2 border-transparent text-muted transition-colors hover:bg-surface-2 hover:text-default",
                item.active && "bg-surface-2 text-default",
              )}
              style={
                item.active
                  ? { borderLeftColor: "var(--nimbus-brand)" }
                  : undefined
              }
            >
              <Icon size={14} aria-hidden />
            </button>
          );
        })}
      </aside>
    );
  }

  return (
    <aside
      aria-label={spec.title}
      data-testid="sub-drawer"
      data-kind={spec.kind}
      data-collapsed="false"
      onDoubleClick={(e) => {
        if (!isInteractiveTarget(e.target)) toggleSubDrawer();
      }}
      className="flex h-full w-64 shrink-0 flex-col border-r border-app bg-surface"
    >
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-app px-3">
        <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
          {spec.title}
        </span>
        <button
          type="button"
          onClick={() => setSubDrawerOpen(false)}
          aria-label="Collapse sub-drawer"
          title="Collapse sub-drawer"
          data-testid="sub-drawer-toggle"
          className="flex h-6 w-6 items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-default"
        >
          <ChevronsLeft size={12} aria-hidden />
        </button>
      </header>
      {spec.kind === "dynamic" && spec.search ? (
        <div className="border-b border-app px-3 py-2">
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={spec.search.placeholder}
            data-testid="sub-drawer-search"
            className="h-7 w-full rounded-md border border-app bg-canvas px-2 text-xs text-default placeholder:text-muted focus:outline-none focus:ring-1 focus:ring-[color:var(--nimbus-brand)]"
          />
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-auto">
        {spec.kind === "static" ? (
          <SubDrawerStaticList items={spec.items} />
        ) : (
          spec.children
        )}
      </div>
    </aside>
  );
}

function isItemActive(
  location: { pathname: string; search?: Record<string, unknown> },
  item: SubDrawerItem<string>,
): boolean {
  const pathMatches =
    location.pathname === item.to ||
    location.pathname.startsWith(`${item.to}/`);
  if (!pathMatches) return false;
  if (!item.search) return true;
  const current = location.search ?? {};
  for (const [key, value] of Object.entries(item.search)) {
    if (current[key] !== value) return false;
  }
  return true;
}

function SubDrawerStaticList({
  items,
}: {
  items: ReadonlyArray<SubDrawerItem<string>>;
}) {
  const location = useRouterState({
    select: (s) => ({
      pathname: s.location.pathname,
      search: s.location.search as Record<string, unknown> | undefined,
    }),
  });
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {items.map((item) => {
        const active = isItemActive(location, item);
        return (
          <li key={item.id}>
            <Link
              to={item.to}
              search={item.search ?? undefined}
              aria-current={active ? "page" : undefined}
              data-testid={`sub-drawer-item-${item.id}`}
              data-active={active ? "true" : "false"}
              className={cn(
                "flex h-8 items-center gap-2 rounded-md border-l-2 border-transparent px-2 text-sm",
                item.disabled
                  ? "pointer-events-none text-muted opacity-60"
                  : active
                    ? "bg-surface-2 text-default"
                    : "text-muted hover:bg-surface-2 hover:text-default",
              )}
              style={
                active ? { borderLeftColor: "var(--nimbus-brand)" } : undefined
              }
            >
              <span className="flex-1 truncate">{item.label}</span>
              {typeof item.count === "number" ? (
                <span className="tabular font-mono text-xs text-muted">
                  {item.count}
                </span>
              ) : null}
            </Link>
          </li>
        );
      })}
    </ul>
  );
}
