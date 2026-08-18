import { fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/developer" },
}));

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({
    select,
  }: {
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

import { useUiStore } from "../store/ui-store";
import { KeyboardContract } from "./keyboard-contract";
import { resolveLensView } from "./system-tenant-lens";

function setPathname(path: string) {
  pathnameRef.current = path;
}

function resetUi() {
  useUiStore.setState({
    paletteOpen: false,
    lensOpen: false,
    paletteOpener: null,
    lensOpener: null,
  });
}

beforeEach(() => {
  setPathname("/developer");
  resetUi();
});

afterEach(() => {
  resetUi();
});

describe("KeyboardContract", () => {
  it("opens the lens on Meta+\\ from a developer pathname and resolves the view", () => {
    setPathname("/developer/compute");
    render(<KeyboardContract />);
    fireEvent.keyDown(window, { key: "\\", metaKey: true });
    expect(useUiStore.getState().lensOpen).toBe(true);
    expect(resolveLensView(pathnameRef.current)).toEqual({
      kind: "functions",
      label: "functions",
    });
  });

  it("opens the lens on Meta+\\ from an operator pathname and resolves the view", () => {
    setPathname("/operator/machines");
    render(<KeyboardContract />);
    fireEvent.keyDown(window, { key: "\\", metaKey: true });
    expect(useUiStore.getState().lensOpen).toBe(true);
    expect(resolveLensView(pathnameRef.current)).toEqual({
      kind: "machines",
      label: "machines",
    });
  });

  it("toggles the palette on Meta+K from any pathname", () => {
    setPathname("/operator/machines");
    render(<KeyboardContract />);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(useUiStore.getState().paletteOpen).toBe(true);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(useUiStore.getState().paletteOpen).toBe(false);
  });

  describe("the / shortcut", () => {
    function mountSearch(kind: string): HTMLInputElement {
      const input = document.createElement("input");
      input.setAttribute("data-inline-search", kind);
      document.body.append(input);
      return input;
    }

    it("focuses the drawer filter when the route has no filter of its own", () => {
      const drawer = mountSearch("drawer");
      render(<KeyboardContract />);
      fireEvent.keyDown(window, { key: "/" });
      expect(document.activeElement).toBe(drawer);
      drawer.remove();
    });

    it("prefers the page filter over the drawer filter", () => {
      // The sub-drawer precedes page content in the DOM, so a plain
      // document-order lookup would always pick the wrong input.
      const drawer = mountSearch("drawer");
      const primary = mountSearch("primary");
      render(<KeyboardContract />);
      fireEvent.keyDown(window, { key: "/" });
      expect(document.activeElement).toBe(primary);
      drawer.remove();
      primary.remove();
    });

    it("leaves / alone while the user is typing", () => {
      const drawer = mountSearch("drawer");
      const typing = document.createElement("input");
      document.body.append(typing);
      typing.focus();
      render(<KeyboardContract />);
      fireEvent.keyDown(typing, { key: "/" });
      expect(document.activeElement).toBe(typing);
      drawer.remove();
      typing.remove();
    });
  });
});
