import { render, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: unknown) => config,
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
  }: {
    to: string;
    children: React.ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a href={to} data-testid={testId} className={className}>
      {children}
    </a>
  ),
}));

vi.mock("nimbus/react", () => ({
  useQuery: () => [],
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import { Route } from "./tenants";

const TenantsPage = (Route as unknown as { component: () => ReactElement })
  .component;

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
  vi.restoreAllMocks();
});

describe("admin/tenants", () => {
  describe("when /api/tenants returns a non-OK response", () => {
    beforeEach(() => {
      globalThis.fetch = vi.fn().mockResolvedValue({
        ok: false,
        status: 404,
        json: async () => ({ error: { message: "Request failed: 404" } }),
      }) as unknown as typeof fetch;
    });

    it("renders the diagnostic envelope with a Retry CTA", async () => {
      render(<TenantsPage />);
      await waitFor(() =>
        expect(
          screen.getByTestId("storage-server-error-envelope"),
        ).toBeInTheDocument(),
      );
      expect(
        screen.getByTestId("storage-server-error-envelope-title"),
      ).toHaveTextContent("Tenants endpoint unavailable");
      expect(
        screen.getByTestId("storage-server-error-envelope-cta"),
      ).toHaveTextContent("Retry");
      expect(screen.getByTestId("storage-server-error")).toHaveTextContent(
        "Request failed: 404",
      );
    });

    it("does not render the table when the diagnostic envelope is shown", async () => {
      render(<TenantsPage />);
      await waitFor(() =>
        expect(
          screen.getByTestId("storage-server-error-envelope"),
        ).toBeInTheDocument(),
      );
      expect(
        screen.queryByTestId("storage-tenants-table"),
      ).not.toBeInTheDocument();
    });
  });

  describe("when /api/tenants returns OK", () => {
    beforeEach(() => {
      globalThis.fetch = vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ tenants: [] }),
      }) as unknown as typeof fetch;
    });

    it("does not render the diagnostic envelope on the happy path", async () => {
      render(<TenantsPage />);
      await waitFor(() =>
        expect(
          screen.queryByTestId("storage-server-error-envelope"),
        ).not.toBeInTheDocument(),
      );
    });
  });
});
