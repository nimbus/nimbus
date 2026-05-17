import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const THEME_STORAGE_KEY = "nimbus-ui:theme";
const PALETTE_STORAGE_KEY = "nimbus-ui:palette";

beforeEach(() => {
  vi.resetModules();
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-palette");
});

afterEach(() => {
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-palette");
});

async function mountAppearancePropagation() {
  const { ThemeController } = await import("../shell/theme-controller");
  const { AppearanceSection } = await import("./appearance-section");
  return render(
    <>
      <ThemeController />
      <AppearanceSection />
    </>,
  );
}

describe("appearance propagation", () => {
  it("clicking each mode button updates the <html data-theme> attribute and persists", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-mode-light"));
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");

    await user.click(screen.getByTestId("appearance-mode-dark"));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");

    await user.click(screen.getByTestId("appearance-mode-system"));
    expect(["light", "dark"]).toContain(document.documentElement.dataset.theme);
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("system");
  });

  it("clicking each palette button updates the <html data-palette> attribute and persists", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-palette-mono"));
    expect(document.documentElement.dataset.palette).toBe("mono");
    expect(window.localStorage.getItem(PALETTE_STORAGE_KEY)).toBe("mono");

    await user.click(screen.getByTestId("appearance-palette-warm"));
    expect(document.documentElement.dataset.palette).toBe("warm");
    expect(window.localStorage.getItem(PALETTE_STORAGE_KEY)).toBe("warm");

    await user.click(screen.getByTestId("appearance-palette-blue"));
    expect(document.documentElement.dataset.palette).toBe("blue");
    expect(window.localStorage.getItem(PALETTE_STORAGE_KEY)).toBe("blue");
  });

  it("mode and palette propagate independently — switching mode does not reset palette", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-palette-warm"));
    await user.click(screen.getByTestId("appearance-mode-dark"));

    expect(document.documentElement.dataset.palette).toBe("warm");
    expect(document.documentElement.dataset.theme).toBe("dark");

    await user.click(screen.getByTestId("appearance-mode-light"));
    expect(document.documentElement.dataset.palette).toBe("warm");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("hydrates DOM data attributes from persisted localStorage on mount", async () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");
    window.localStorage.setItem(PALETTE_STORAGE_KEY, "mono");
    await mountAppearancePropagation();

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.dataset.palette).toBe("mono");
  });

  it("marks the active mode and palette buttons via aria-checked + data-active", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-mode-light"));
    await user.click(screen.getByTestId("appearance-palette-warm"));

    const lightMode = screen.getByTestId("appearance-mode-light");
    const darkMode = screen.getByTestId("appearance-mode-dark");
    expect(lightMode.getAttribute("aria-checked")).toBe("true");
    expect(lightMode.dataset.active).toBe("true");
    expect(darkMode.getAttribute("aria-checked")).toBe("false");
    expect(darkMode.dataset.active).toBe("false");

    const warmPalette = screen.getByTestId("appearance-palette-warm");
    const monoPalette = screen.getByTestId("appearance-palette-mono");
    expect(warmPalette.getAttribute("aria-checked")).toBe("true");
    expect(warmPalette.dataset.active).toBe("true");
    expect(monoPalette.getAttribute("aria-checked")).toBe("false");
    expect(monoPalette.dataset.active).toBe("false");
  });
});
