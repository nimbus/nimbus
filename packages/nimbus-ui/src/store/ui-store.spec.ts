import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const THEME_STORAGE_KEY = "nimbus-ui:theme";
const PALETTE_STORAGE_KEY = "nimbus-ui:palette";
const ACTIVE_TENANT_KEY = "nimbus-ui:active-tenant";

beforeEach(() => {
  vi.resetModules();
  window.localStorage.clear();
});

afterEach(() => {
  window.localStorage.clear();
});

describe("ui-store", () => {
  it("defaults to system mode when nothing is persisted", async () => {
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().themeMode).toBe("system");
    expect(["light", "dark"]).toContain(useUiStore.getState().theme);
  });

  it("hydrates from the persisted plain-string key", async () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().themeMode).toBe("dark");
    expect(useUiStore.getState().theme).toBe("dark");
  });

  it("falls back to system when the persisted value is malformed", async () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "neon");
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().themeMode).toBe("system");
  });

  it("setThemeMode writes through and updates the resolved theme", async () => {
    const { useUiStore } = await import("./ui-store");
    useUiStore.getState().setThemeMode("light");
    expect(useUiStore.getState().themeMode).toBe("light");
    expect(useUiStore.getState().theme).toBe("light");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
  });

  it("cycleThemeMode rotates Light -> Dark -> System", async () => {
    const { useUiStore } = await import("./ui-store");
    useUiStore.getState().setThemeMode("light");
    useUiStore.getState().cycleThemeMode();
    expect(useUiStore.getState().themeMode).toBe("dark");
    useUiStore.getState().cycleThemeMode();
    expect(useUiStore.getState().themeMode).toBe("system");
    useUiStore.getState().cycleThemeMode();
    expect(useUiStore.getState().themeMode).toBe("light");
  });

  it("setPaletteOpen captures the opener and restores focus on close", async () => {
    const { useUiStore } = await import("./ui-store");
    const button = document.createElement("button");
    document.body.appendChild(button);
    const focusSpy = vi.spyOn(button, "focus");
    useUiStore.getState().setPaletteOpen(true, button);
    expect(useUiStore.getState().paletteOpen).toBe(true);
    expect(useUiStore.getState().paletteOpener).toBe(button);
    useUiStore.getState().setPaletteOpen(false);
    expect(useUiStore.getState().paletteOpen).toBe(false);
    await new Promise((r) => queueMicrotask(() => r(undefined)));
    expect(focusSpy).toHaveBeenCalled();
    button.remove();
  });

  it("defaults to blue palette when nothing is persisted", async () => {
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().palette).toBe("blue");
  });

  it("hydrates palette from the persisted key", async () => {
    window.localStorage.setItem(PALETTE_STORAGE_KEY, "warm");
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().palette).toBe("warm");
  });

  it("falls back to blue when the persisted palette is malformed", async () => {
    window.localStorage.setItem(PALETTE_STORAGE_KEY, "rainbow");
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().palette).toBe("blue");
  });

  it("setPalette writes through and updates state", async () => {
    const { useUiStore } = await import("./ui-store");
    useUiStore.getState().setPalette("mono");
    expect(useUiStore.getState().palette).toBe("mono");
    expect(window.localStorage.getItem(PALETTE_STORAGE_KEY)).toBe("mono");
  });

  it("defaults activeTenant to null when nothing is persisted", async () => {
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().activeTenant).toBeNull();
  });

  it("hydrates activeTenant from the persisted key", async () => {
    window.localStorage.setItem(ACTIVE_TENANT_KEY, "acme");
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().activeTenant).toBe("acme");
  });

  it("setActiveTenant writes through and clears on null", async () => {
    const { useUiStore } = await import("./ui-store");
    useUiStore.getState().setActiveTenant("acme");
    expect(useUiStore.getState().activeTenant).toBe("acme");
    expect(window.localStorage.getItem(ACTIVE_TENANT_KEY)).toBe("acme");
    useUiStore.getState().setActiveTenant(null);
    expect(useUiStore.getState().activeTenant).toBeNull();
    expect(window.localStorage.getItem(ACTIVE_TENANT_KEY)).toBeNull();
  });

  it("setLensOpen mirrors palette behavior on its own opener slot", async () => {
    const { useUiStore } = await import("./ui-store");
    const button = document.createElement("button");
    document.body.appendChild(button);
    useUiStore.getState().setLensOpen(true, button);
    expect(useUiStore.getState().lensOpen).toBe(true);
    expect(useUiStore.getState().lensOpener).toBe(button);
    useUiStore.getState().setLensOpen(false);
    expect(useUiStore.getState().lensOpen).toBe(false);
    expect(useUiStore.getState().lensOpener).toBeNull();
    button.remove();
  });
});
