import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  Link: ({
    to,
    children,
    "data-testid": testId,
  }: {
    to: string;
    children: React.ReactNode;
    "data-testid"?: string;
  }) => (
    <a href={to} data-testid={testId}>
      {children}
    </a>
  ),
}));

const { contributeMock } = vi.hoisted(() => ({ contributeMock: vi.fn() }));

vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: (spec: unknown) => contributeMock(spec),
}));

import { routeComponent } from "../../test/route-internals";
import { Route, TENANT_SETTINGS_PLANNED } from "./settings";

const SettingsPage = routeComponent(Route);

beforeEach(() => {
  contributeMock.mockClear();
});

describe("tenant settings", () => {
  // A static menu lists only pages that exist. No tenant sub-page is built,
  // so there is no menu: the shell's layout is a pass-through here.
  it("contributes no sub-panel", () => {
    render(<SettingsPage />);
    expect(contributeMock).not.toHaveBeenCalled();
  });

  it("carries no section in its search", () => {
    const config = Route as unknown as Record<string, unknown>;
    expect(config.validateSearch).toBeUndefined();
    expect(config.beforeLoad).toBeUndefined();
  });

  it("says once that no tenant setting exists and names the planned ones", () => {
    render(<SettingsPage />);
    expect(screen.getByTestId("page-settings")).toBeTruthy();
    const body = screen.getByTestId("settings-empty-body").textContent ?? "";
    for (const planned of TENANT_SETTINGS_PLANNED) {
      expect(body).toContain(planned);
    }
    expect(screen.queryAllByTestId(/-unavailable$/)).toHaveLength(0);
  });

  it("offers the operator settings as the way on", () => {
    render(<SettingsPage />);
    expect(screen.getByTestId("settings-empty-cta")).toHaveAttribute(
      "href",
      "/operator/settings",
    );
  });

  it("renders its subtitle through the shared PageHeader", () => {
    render(<SettingsPage />);
    const subtitle = screen
      .getByTestId("page-settings")
      .querySelector('[data-slot="page-subtitle"]');
    expect(subtitle?.textContent).toContain("active tenant");
  });
});
