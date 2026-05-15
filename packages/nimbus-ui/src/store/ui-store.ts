import { create } from "zustand";

export type Theme = "light" | "dark";

type UiState = {
  paletteOpen: boolean;
  lensOpen: boolean;
  actionMenuOpen: boolean;
  theme: Theme;
  paletteOpener: HTMLElement | null;
  lensOpener: HTMLElement | null;
  setPaletteOpen: (open: boolean, opener?: HTMLElement | null) => void;
  setLensOpen: (open: boolean, opener?: HTMLElement | null) => void;
  setActionMenuOpen: (open: boolean) => void;
  toggleTheme: () => void;
  setTheme: (theme: Theme) => void;
};

const THEME_STORAGE_KEY = "nimbus-ui:theme";

function initialTheme(): Theme {
  if (typeof window === "undefined") return "dark";
  const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia?.("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

export const useUiStore = create<UiState>((set, get) => ({
  paletteOpen: false,
  lensOpen: false,
  actionMenuOpen: false,
  theme: initialTheme(),
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
  toggleTheme: () => {
    const next: Theme = get().theme === "dark" ? "light" : "dark";
    persistTheme(next);
    set({ theme: next });
  },
  setTheme: (theme) => {
    persistTheme(theme);
    set({ theme });
  },
}));

function persistTheme(theme: Theme) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(THEME_STORAGE_KEY, theme);
}
