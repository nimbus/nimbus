import { create } from "zustand";

export type ThemeMode = "light" | "dark" | "system";
export type Theme = "light" | "dark";
export type NavView = "developer" | "operator";

type UiState = {
  paletteOpen: boolean;
  lensOpen: boolean;
  actionMenuOpen: boolean;
  themeMode: ThemeMode;
  theme: Theme;
  lastView: NavView;
  sidebarCollapsed: boolean;
  activeTenant: string | null;
  paletteOpener: HTMLElement | null;
  lensOpener: HTMLElement | null;
  setPaletteOpen: (open: boolean, opener?: HTMLElement | null) => void;
  setLensOpen: (open: boolean, opener?: HTMLElement | null) => void;
  setActionMenuOpen: (open: boolean) => void;
  setThemeMode: (mode: ThemeMode) => void;
  setLastView: (view: NavView) => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebar: () => void;
  setActiveTenant: (tenant: string | null) => void;
  cycleThemeMode: () => void;
};

const THEME_STORAGE_KEY = "nimbus-ui:theme";
const LAST_VIEW_STORAGE_KEY = "nimbus-ui:last-view";
const LAST_ROUTE_STORAGE_PREFIX = "nimbus-ui:last-route:";
const SIDEBAR_COLLAPSED_KEY = "nimbus-ui:sidebar-collapsed";
const ACTIVE_TENANT_KEY = "nimbus-ui:active-tenant";
const SYSTEM_DARK_QUERY = "(prefers-color-scheme: dark)";

function readSystemTheme(): Theme {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia?.(SYSTEM_DARK_QUERY).matches ? "dark" : "light";
}

function readStoredMode(): ThemeMode {
  if (typeof window === "undefined") return "system";
  const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
  if (stored === "light" || stored === "dark" || stored === "system") {
    return stored;
  }
  return "system";
}

export function readLastView(): NavView {
  if (typeof window === "undefined") return "developer";
  const stored = window.localStorage.getItem(LAST_VIEW_STORAGE_KEY);
  return stored === "operator" ? "operator" : "developer";
}

export function readLastRouteForView(view: NavView): string | null {
  if (typeof window === "undefined") return null;
  const stored = window.localStorage.getItem(
    `${LAST_ROUTE_STORAGE_PREFIX}${view}`,
  );
  return stored?.startsWith(`/${view}`) ? stored : null;
}

export function persistLastRouteForView(view: NavView, pathname: string) {
  if (typeof window === "undefined") return;
  if (!pathname.startsWith(`/${view}`)) return;
  window.localStorage.setItem(`${LAST_ROUTE_STORAGE_PREFIX}${view}`, pathname);
}

function resolveTheme(mode: ThemeMode): Theme {
  return mode === "system" ? readSystemTheme() : mode;
}

export function readSidebarCollapsed(): boolean {
  if (typeof window === "undefined") return false;
  return window.localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true";
}

export function readActiveTenant(): string | null {
  if (typeof window === "undefined") return null;
  const stored = window.localStorage.getItem(ACTIVE_TENANT_KEY);
  return stored && stored.length > 0 ? stored : null;
}

export function persistActiveTenant(tenant: string | null) {
  if (typeof window === "undefined") return;
  if (tenant === null) {
    window.localStorage.removeItem(ACTIVE_TENANT_KEY);
  } else {
    window.localStorage.setItem(ACTIVE_TENANT_KEY, tenant);
  }
}

const initialMode = readStoredMode();
const initialLastView = readLastView();
const initialSidebarCollapsed = readSidebarCollapsed();
const initialActiveTenant = readActiveTenant();

export const useUiStore = create<UiState>((set, get) => ({
  paletteOpen: false,
  lensOpen: false,
  actionMenuOpen: false,
  themeMode: initialMode,
  theme: resolveTheme(initialMode),
  lastView: initialLastView,
  sidebarCollapsed: initialSidebarCollapsed,
  activeTenant: initialActiveTenant,
  paletteOpener: null,
  lensOpener: null,
  setPaletteOpen: (open, opener) =>
    set((state) => {
      if (open) {
        return {
          paletteOpen: true,
          paletteOpener:
            opener ?? (document.activeElement as HTMLElement | null) ?? null,
        };
      }
      const restore = state.paletteOpener;
      queueMicrotask(() => restore?.focus?.());
      return { paletteOpen: false, paletteOpener: null };
    }),
  setLensOpen: (open, opener) =>
    set((state) => {
      if (open) {
        return {
          lensOpen: true,
          lensOpener:
            opener ?? (document.activeElement as HTMLElement | null) ?? null,
        };
      }
      const restore = state.lensOpener;
      queueMicrotask(() => restore?.focus?.());
      return { lensOpen: false, lensOpener: null };
    }),
  setActionMenuOpen: (open) => set({ actionMenuOpen: open }),
  setThemeMode: (mode) => {
    persistMode(mode);
    set({ themeMode: mode, theme: resolveTheme(mode) });
  },
  setLastView: (view) => {
    persistLastView(view);
    set({ lastView: view });
  },
  setSidebarCollapsed: (collapsed) => {
    persistSidebarCollapsed(collapsed);
    set({ sidebarCollapsed: collapsed });
  },
  toggleSidebar: () => {
    const next = !get().sidebarCollapsed;
    persistSidebarCollapsed(next);
    set({ sidebarCollapsed: next });
  },
  setActiveTenant: (tenant) => {
    persistActiveTenant(tenant);
    set({ activeTenant: tenant });
  },
  cycleThemeMode: () => {
    const order: ThemeMode[] = ["light", "dark", "system"];
    const current = get().themeMode;
    const next = order[(order.indexOf(current) + 1) % order.length];
    persistMode(next);
    set({ themeMode: next, theme: resolveTheme(next) });
  },
}));

function persistMode(mode: ThemeMode) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(THEME_STORAGE_KEY, mode);
}

function persistLastView(view: NavView) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(LAST_VIEW_STORAGE_KEY, view);
}

function persistSidebarCollapsed(collapsed: boolean) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(
    SIDEBAR_COLLAPSED_KEY,
    collapsed ? "true" : "false",
  );
}

if (typeof window !== "undefined" && window.matchMedia) {
  const mql = window.matchMedia(SYSTEM_DARK_QUERY);
  const listener = () => {
    if (useUiStore.getState().themeMode === "system") {
      useUiStore.setState({ theme: readSystemTheme() });
    }
  };
  mql.addEventListener?.("change", listener);
}
