import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  useRouterState: () => ({ location: { pathname: "/developer/storage" } }),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: () => [],
}));

import { useUiStore } from "../store/ui-store";
import { resolveLensView, SystemTenantLens } from "./system-tenant-lens";

afterEach(() => {
  // Unmount before touching the store: this hook runs before the shared
  // cleanup in setup.ts, so a bare setState would re-render a live tree
  // outside act().
  cleanup();
  useUiStore.setState({ lensOpen: false });
});

describe("resolveLensView", () => {
  it("maps /developer/storage to the tables view", () => {
    expect(resolveLensView("/developer/storage")).toEqual({
      kind: "tables",
      label: "tables",
    });
  });

  it("maps /developer/compute to the functions view", () => {
    expect(resolveLensView("/developer/compute")).toEqual({
      kind: "functions",
      label: "functions",
    });
  });

  it("maps /developer/observability to the runs view", () => {
    expect(resolveLensView("/developer/observability")).toEqual({
      kind: "runs",
      label: "runs",
    });
  });

  it("maps /operator/machines to the machines view", () => {
    expect(resolveLensView("/operator/machines")).toEqual({
      kind: "machines",
      label: "machines",
    });
  });

  it("maps /operator/network to the listeners view", () => {
    expect(resolveLensView("/operator/network")).toEqual({
      kind: "listeners",
      label: "listeners",
    });
  });

  it("falls back to system.status on /operator/settings", () => {
    expect(resolveLensView("/operator/settings")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });

  it("falls back to system.status on /developer/settings", () => {
    expect(resolveLensView("/developer/settings")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });

  it("falls back to system.status on bare /developer", () => {
    expect(resolveLensView("/developer")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });

  it("falls back to system.status on bare /operator", () => {
    expect(resolveLensView("/operator")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });

  it("matches the observability view across personas", () => {
    expect(resolveLensView("/developer/observability").kind).toBe("runs");
    expect(resolveLensView("/operator/observability").kind).toBe("runs");
  });

  it("strips query strings and hash before matching", () => {
    expect(resolveLensView("/developer/storage?tenant=demo").kind).toBe(
      "tables",
    );
    expect(resolveLensView("/operator/machines#detail").kind).toBe("machines");
  });

  it("returns system.status for an unrelated pathname", () => {
    expect(resolveLensView("/unknown/route")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });
});

/**
 * DESIGN.md §Spacing And Shape: "Icon button: 32px square, 36px on touch
 * surfaces."
 *
 * `p-1` around a 16px glyph gave a 24px button -- the smallest target in the
 * shell, and the only pointer route out of a panel that covers half the
 * viewport.
 */
describe("SystemTenantLens close button", () => {
  it("sizes the close button 32px square", () => {
    useUiStore.setState({ lensOpen: true });
    render(<SystemTenantLens />);
    const classes = screen.getByTestId("lens-close").className.split(" ");
    expect(classes).toEqual(expect.arrayContaining(["h-8", "w-8"]));
    // Padding cannot stay: 4px around a 16px glyph inside a fixed 32px box
    // shrinks the glyph's own box instead of growing the target.
    expect(classes).not.toContain("p-1");
  });

  it("centres the glyph in the larger box", () => {
    useUiStore.setState({ lensOpen: true });
    render(<SystemTenantLens />);
    const classes = screen.getByTestId("lens-close").className.split(" ");
    // Sizing the box without centring parks the X in its top-left corner.
    expect(classes).toEqual(
      expect.arrayContaining(["flex", "items-center", "justify-center"]),
    );
  });
});
