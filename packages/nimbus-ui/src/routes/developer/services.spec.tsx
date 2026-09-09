import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { navigateMock, invalidateMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  invalidateMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => navigateMock,
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

const { serviceApiMock, toastMock } = vi.hoisted(() => ({
  serviceApiMock: { start: vi.fn(), stop: vi.fn(), restart: vi.fn() },
  toastMock: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
}));

vi.mock("../../lib/api-mutations", () => ({ services: serviceApiMock }));
vi.mock("sonner", () => ({ toast: toastMock }));

import { ServicesLoaderError } from "../../components/service-loader-errors";
import type { ServiceDoc } from "../../lib/types/service";
import { useUiStore } from "../../store/ui-store";
import { routeLoader, routeLoaderDeps } from "../../test/route-internals";
import { Route, ServicesTable } from "./services";

type LoaderArgs = {
  deps: { activeTenant: string | null };
};

type LoaderResult = {
  services: unknown[];
  activeTenant: string | null;
};

const loaderDeps = routeLoaderDeps<{ activeTenant: string | null }>(Route);
const loader = routeLoader<LoaderArgs, LoaderResult>(Route);

beforeEach(() => {
  nimbusQueryMock.mockReset();
  navigateMock.mockReset();
  invalidateMock.mockReset();
  for (const fn of Object.values(serviceApiMock)) fn.mockReset();
  toastMock.success.mockReset();
  toastMock.error.mockReset();
  useUiStore.setState({ activeTenant: null });
});

afterEach(() => {
  useUiStore.setState({ activeTenant: null });
  vi.restoreAllMocks();
});

describe("app/services loaderDeps + loader", () => {
  it("loaderDeps snapshots activeTenant from the Zustand store", () => {
    useUiStore.setState({ activeTenant: "acme" });
    expect(loaderDeps()).toEqual({ activeTenant: "acme" });
  });

  it("loaderDeps returns activeTenant=null when no tenant is selected", () => {
    expect(loaderDeps()).toEqual({ activeTenant: null });
  });

  it("queries services scoped to deps.activeTenant", async () => {
    const services = [{ _id: "s1", name: "api", tenantId: "acme" }];
    nimbusQueryMock.mockResolvedValue(services);

    const result = await loader({
      deps: { activeTenant: "acme" },
    });

    expect(nimbusQueryMock.mock.calls[0]?.[1]).toMatchObject({
      tenantId: "acme",
      machineId: null,
      state: null,
      limit: 200,
    });
    expect(result.services).toEqual(services);
    expect(result.activeTenant).toBe("acme");
  });

  it("passes activeTenant=null when deps.activeTenant is null", async () => {
    nimbusQueryMock.mockResolvedValue([]);

    const result = await loader({
      deps: { activeTenant: null },
    });

    expect(nimbusQueryMock.mock.calls[0]?.[1]?.tenantId).toBeNull();
    expect(result.activeTenant).toBeNull();
  });
});

describe("app/services errorComponent", () => {
  it("renders the diagnostic envelope with the loader-error message and a Retry CTA wired to reset", async () => {
    const reset = vi.fn();
    render(
      <ServicesLoaderError error={new Error("convex down")} reset={reset} />,
    );
    expect(
      screen.getByTestId("storage-server-error-envelope"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("storage-server-error-envelope-title"),
    ).toHaveTextContent("Services endpoint unavailable");
    expect(
      screen.getByTestId("storage-server-error-envelope-cta"),
    ).toHaveTextContent("Retry");
    expect(screen.getByTestId("storage-server-error")).toHaveTextContent(
      "convex down",
    );
    screen.getByTestId("storage-server-error-envelope-cta").click();
    expect(reset).toHaveBeenCalledTimes(1);
  });
});

describe("app/services empty-state copy", () => {
  // Markdown backticks were being written into strings that land in a text
  // node, so the user read a literal ` around the command. Commands are
  // marked up as <code>, never as markdown.
  it("marks the compose command up as <code> and leaks no backticks", () => {
    render(
      <ServicesTable services={[]} activeTenant="acme" showTenantColumn />,
    );

    const body = screen.getByTestId("services-empty-body");
    expect(body.textContent).not.toContain("`");
    expect(body.textContent).toContain("run nimbus compose up.");

    const commands = Array.from(body.querySelectorAll("code")).map(
      (el) => el.textContent,
    );
    expect(commands).toEqual(["compose.yaml", "nimbus compose up"]);
  });

  it("keeps the multi-word command on one line", () => {
    render(
      <ServicesTable services={[]} activeTenant="acme" showTenantColumn />,
    );

    // A multi-word command that wraps renders as two separate boxed
    // fragments once <code> carries a background, so it must not wrap.
    const command = Array.from(
      screen.getByTestId("services-empty-body").querySelectorAll("code"),
    ).find((el) => el.textContent === "nimbus compose up");
    expect(command?.className).toContain("whitespace-nowrap");
  });

  it("leaks no backticks in the all-tenant variant", () => {
    render(
      <ServicesTable services={[]} activeTenant={null} showTenantColumn />,
    );

    const body = screen.getByTestId("services-empty-body");
    expect(body.textContent).not.toContain("`");
    expect(body.querySelectorAll("code")).toHaveLength(0);
  });
});

const READY = {
  _id: "svc-api",
  name: "api",
  tenantId: "acme",
  kind: "container",
  state: "ready",
  sourceGeneration: "3",
} as unknown as ServiceDoc;

const STOPPED = {
  _id: "svc-web",
  name: "web",
  tenantId: "acme",
  kind: "microvm",
  state: "stopped",
} as unknown as ServiceDoc;

function renderTable(services: ServiceDoc[]) {
  return render(
    <ServicesTable
      services={services}
      activeTenant="acme"
      showTenantColumn={false}
    />,
  );
}

describe("ServicesTable row actions", () => {
  it("offers stop and restart on a ready service, and start on a stopped one", () => {
    renderTable([READY, STOPPED]);

    fireEvent.contextMenu(screen.getByTestId("services-row-api"));
    const menu = screen.getByTestId("services-row-menu");
    expect(within(menu).getByTestId("services-row-menu-open")).toBeTruthy();
    expect(within(menu).getByTestId("services-row-menu-stop")).toBeTruthy();
    expect(within(menu).getByTestId("services-row-menu-restart")).toBeTruthy();
    expect(within(menu).queryByTestId("services-row-menu-start")).toBeNull();
    expect(within(menu).getByTestId("services-row-menu-logs")).toBeTruthy();
    fireEvent.keyDown(menu, { key: "Escape" });

    fireEvent.click(screen.getByTestId("services-row-actions-web"));
    const stoppedMenu = screen.getByTestId("services-row-menu");
    expect(
      within(stoppedMenu).getByTestId("services-row-menu-start"),
    ).toBeTruthy();
    expect(
      within(stoppedMenu).queryByTestId("services-row-menu-stop"),
    ).toBeNull();
  });

  it("sends stop to the lifecycle route and shows the row as stopping until it settles", async () => {
    let settle: (value: { ok: true; data: unknown }) => void = () => {};
    serviceApiMock.stop.mockReturnValue(
      new Promise((resolve) => {
        settle = resolve;
      }),
    );
    renderTable([READY]);

    fireEvent.contextMenu(screen.getByTestId("services-row-api"));
    fireEvent.click(
      within(screen.getByTestId("services-row-menu")).getByTestId(
        "services-row-menu-stop",
      ),
    );

    expect(serviceApiMock.stop).toHaveBeenCalledWith("acme", "api");
    await waitFor(() =>
      expect(screen.getByTestId("services-state-api")).toHaveAttribute(
        "data-state",
        "stopping",
      ),
    );

    settle({ ok: true, data: { state: "stopping" } });
    await waitFor(() => expect(invalidateMock).toHaveBeenCalledTimes(1));
    expect(toastMock.success).toHaveBeenCalledWith("Stop sent to api");
    await waitFor(() =>
      expect(screen.getByTestId("services-state-api")).toHaveAttribute(
        "data-state",
        "ready",
      ),
    );
  });

  it("sends restart with the row's source generation and a request id", async () => {
    serviceApiMock.restart.mockResolvedValue({ ok: true, data: {} });
    renderTable([READY]);

    fireEvent.contextMenu(screen.getByTestId("services-row-api"));
    fireEvent.click(
      within(screen.getByTestId("services-row-menu")).getByTestId(
        "services-row-menu-restart",
      ),
    );

    await waitFor(() =>
      expect(serviceApiMock.restart).toHaveBeenCalledTimes(1),
    );
    const [tenant, name, request] = serviceApiMock.restart.mock.calls[0] ?? [];
    expect(tenant).toBe("acme");
    expect(name).toBe("api");
    expect(request).toMatchObject({ sourceGeneration: 3 });
    expect(typeof request.requestId).toBe("string");
    expect(request.requestId.length).toBeGreaterThan(8);
  });

  it("keeps the row on its real state and shows the refusal when the route says no", async () => {
    serviceApiMock.start.mockResolvedValue({
      ok: false,
      error: "service_not_found",
      status: 404,
    });
    renderTable([STOPPED]);

    fireEvent.contextMenu(screen.getByTestId("services-row-web"));
    fireEvent.click(
      within(screen.getByTestId("services-row-menu")).getByTestId(
        "services-row-menu-start",
      ),
    );

    await waitFor(() =>
      expect(screen.getByTestId("services-row-error-web")).toHaveTextContent(
        "service_not_found",
      ),
    );
    expect(screen.getByTestId("services-state-web")).toHaveAttribute(
      "data-state",
      "stopped",
    );
    expect(toastMock.error).toHaveBeenCalledWith("Start refused for web", {
      description: "service_not_found",
    });
    expect(invalidateMock).not.toHaveBeenCalled();
  });

  it("opens the service on row activation and the observability logs from the menu", () => {
    renderTable([READY]);

    fireEvent.click(screen.getByTestId("services-row-api"));
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/developer/services/$service",
      params: { service: "svc-api" },
    });

    fireEvent.contextMenu(screen.getByTestId("services-row-api"));
    fireEvent.click(
      within(screen.getByTestId("services-row-menu")).getByTestId(
        "services-row-menu-logs",
      ),
    );
    expect(navigateMock).toHaveBeenLastCalledWith({
      to: "/developer/observability",
      search: { tab: "logs", source: "service", tenant: "acme" },
    });
  });
});
