import { Link, useRouterState } from "@tanstack/react-router";
import { X } from "lucide-react";
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

export type SubDrawerItem = {
  id: string;
  label: string;
  to: string;
  search?: Record<string, unknown>;
  description?: string;
  disabled?: boolean;
  count?: number | null;
};

export type SubDrawerSpec =
  | { kind: "static"; title: string; items: SubDrawerItem[] }
  | {
      kind: "dynamic";
      title: string;
      search?: { placeholder: string };
      children: ReactNode;
    };

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
  const spec = ctx?.spec ?? null;
  const search = ctx?.search ?? "";
  const setSearch = ctx?.setSearch ?? (() => {});
  if (!spec || !open) return null;
  return (
    <aside
      aria-label={spec.title}
      data-testid="sub-drawer"
      data-kind={spec.kind}
      className="flex h-full w-64 shrink-0 flex-col border-r border-app bg-surface"
    >
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-app px-3">
        <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
          {spec.title}
        </span>
        <button
          type="button"
          onClick={() => setSubDrawerOpen(false)}
          aria-label="Close sub-drawer"
          title="Close sub-drawer"
          data-testid="sub-drawer-close"
          className="flex h-6 w-6 items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-default"
        >
          <X size={12} aria-hidden />
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
            className="h-7 w-full rounded-md border border-app bg-app px-2 text-xs text-default placeholder:text-muted focus:outline-none focus:ring-1 focus:ring-[color:var(--color-brand)]"
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
  item: SubDrawerItem,
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

function SubDrawerStaticList({ items }: { items: SubDrawerItem[] }) {
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
                active ? { borderLeftColor: "var(--color-brand)" } : undefined
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
