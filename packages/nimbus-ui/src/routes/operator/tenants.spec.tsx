import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { loaderDataRef, invalidateMock } = vi.hoisted(() => ({
  loaderDataRef: { current: null as unknown },
  invalidateMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => ({
    ...config,
    useLoaderData: () => loaderDataRef.current,
  }),
  useRouter: () => ({ invalidate: invalidateMock }),
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

const { nimbusQueryMock } = vi.hoisted(() => ({
  nimbusQueryMock: vi.fn(),
}));

vi.mock("../../lib/nimbus-client", () => ({
  getNimbusClient: () => ({ query: nimbusQueryMock }),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import { Route } from "./tenants";
import { routeComponent, routeLoader } from "../../test/route-internals";

type LoaderResult =
  | { kind: "ok"; tenants: string[]; tables: unknown[] }
  | { kind: "error"; message: string };

const TenantsPage = routeComponent(Route);
const loader = routeLoader<{ abortController: AbortController }, LoaderResult>(
  Route,
);

beforeEach(() => {
  loaderDataRef.current = null;
  invalidateMock.mockReset();
  nimbusQueryMock.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("admin/tenants loader", () => {
  it("returns kind=error when /api/tenants is non-OK", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 404,
        json: async () => ({ error: { message: "Request failed: 404" } }),
      }),
    );

    const result = await loader({ abortController: new AbortController() });
    expect(result.kind).toBe("error");
    if (result.kind === "error") {
      expect(result.message).toContain("non-OK");
    }
    expect(nimbusQueryMock).not.toHaveBeenCalled();
  });

  it("returns kind=error when fetch throws (e.g. abort)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValue(new Error("network down")),
    );

    const result = await loader({ abortController: new AbortController() });
    expect(result.kind).toBe("error");
    if (result.kind === "error") {
      expect(result.message).toBe("network down");
    }
  });

  it("returns kind=ok with sorted tenants + tables on the happy path", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ tenants: ["beta", "alpha"] }),
      }),
    );
    nimbusQueryMock.mockResolvedValue([
      { _id: "t1", tenantId: "alpha", name: "users", rowCount: 3 },
    ]);

    const result = await loader({ abortController: new AbortController() });
    expect(result.kind).toBe("ok");
    if (result.kind === "ok") {
      expect(result.tenants).toEqual(["alpha", "beta"]);
      expect(result.tables).toHaveLength(1);
    }
  });

  it("forwards abort signal to fetch", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ tenants: [] }),
    });
    vi.stubGlobal("fetch", fetchMock);
    nimbusQueryMock.mockResolvedValue([]);

    const controller = new AbortController();
    await loader({ abortController: controller });
    const call = fetchMock.mock.calls[0];
    expect(call?.[0]).toBe("/api/tenants");
    expect(call?.[1]).toMatchObject({ signal: controller.signal });
  });
});

describe("admin/tenants render", () => {
  it("renders the diagnostic envelope with a Retry CTA on error", () => {
    loaderDataRef.current = {
      kind: "error",
      message: "Request failed: 404",
    };
    render(<TenantsPage />);
    expect(
      screen.getByTestId("storage-server-error-envelope"),
    ).toBeInTheDocument();
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

  it("does not render the table when the diagnostic envelope is shown", () => {
    loaderDataRef.current = {
      kind: "error",
      message: "Tenants endpoint returned a non-OK response.",
    };
    render(<TenantsPage />);
    expect(
      screen.queryByTestId("storage-tenants-table"),
    ).not.toBeInTheDocument();
  });

  it("does not render the diagnostic envelope on the happy path", () => {
    loaderDataRef.current = { kind: "ok", tenants: [], tables: [] };
    render(<TenantsPage />);
    expect(
      screen.queryByTestId("storage-server-error-envelope"),
    ).not.toBeInTheDocument();
  });
});
