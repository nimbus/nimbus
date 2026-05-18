import { fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/app" },
}));

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({
    select,
  }: {
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

import { KeyboardContract } from "./keyboard-contract";
import { useUiStore } from "../store/ui-store";

function setPathname(path: string) {
  pathnameRef.current = path;
}

function resetUi() {
  useUiStore.setState({
    paletteOpen: false,
    lensOpen: false,
    actionMenuOpen: false,
    paletteOpener: null,
    lensOpener: null,
  });
}

beforeEach(() => {
  setPathname("/app");
  resetUi();
});

afterEach(() => {
  resetUi();
});

describe("KeyboardContract", () => {
  it("opens the lens on Meta+\\ from a developer pathname", () => {
    setPathname("/app/compute");
    render(<KeyboardContract />);
    fireEvent.keyDown(window, { key: "\\", metaKey: true });
    expect(useUiStore.getState().lensOpen).toBe(true);
  });

  it("does not open the lens on Meta+\\ from an operator pathname", () => {
    setPathname("/admin/machines");
    render(<KeyboardContract />);
    fireEvent.keyDown(window, { key: "\\", metaKey: true });
    expect(useUiStore.getState().lensOpen).toBe(false);
  });

  it("toggles the palette on Meta+K from any pathname", () => {
    setPathname("/admin/machines");
    render(<KeyboardContract />);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(useUiStore.getState().paletteOpen).toBe(true);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(useUiStore.getState().paletteOpen).toBe(false);
  });
});
