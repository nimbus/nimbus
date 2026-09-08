import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const THEME_STORAGE_KEY = "nimbus-ui:theme";

beforeEach(() => {
  vi.resetModules();
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

afterEach(() => {
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
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
  it("clicking each mode radio updates the <html data-theme> attribute and persists", async () => {
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

  it("hydrates <html data-theme> from persisted localStorage on mount", async () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");
    await mountAppearancePropagation();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("marks the active mode via aria-checked + data-active", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-mode-light"));

    const lightMode = screen.getByTestId("appearance-mode-light");
    const darkMode = screen.getByTestId("appearance-mode-dark");
    expect(lightMode.getAttribute("aria-checked")).toBe("true");
    expect(lightMode.dataset.active).toBe("true");
    expect(darkMode.getAttribute("aria-checked")).toBe("false");
    expect(darkMode.dataset.active).toBe("false");
  });
});

/* One palette (neutral grounds, one amber accent). The section offers the
   three modes and nothing else, and the controller never stamps a palette
   attribute, so no stylesheet selector can key on one. */
describe("one palette", () => {
  it("offers exactly light, dark and system", async () => {
    await mountAppearancePropagation();
    const modes = screen
      .getAllByRole("radio")
      .map((radio) => radio.getAttribute("data-testid"));
    expect(modes).toEqual([
      "appearance-mode-light",
      "appearance-mode-dark",
      "appearance-mode-system",
    ]);
  });

  it("renders no palette control and stamps no data-palette", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();
    expect(screen.queryByTestId("appearance-palette")).toBeNull();
    expect(document.querySelector("[data-palette]")).toBeNull();
    await user.click(screen.getByTestId("appearance-mode-dark"));
    await user.click(screen.getByTestId("appearance-mode-light"));
    expect(document.documentElement.hasAttribute("data-palette")).toBe(false);
    // The theme is the only preference the section persists.
    expect(window.localStorage.length).toBe(1);
    expect(window.localStorage.key(0)).toBe(THEME_STORAGE_KEY);
  });
});
