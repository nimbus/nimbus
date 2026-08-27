import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
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

import { ServicesLoaderError } from "../../components/service-loader-errors";
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
