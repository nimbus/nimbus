import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

const { toastMock } = vi.hoisted(() => ({
  toastMock: Object.assign(vi.fn(), { error: vi.fn() }),
}));

vi.mock("sonner", () => ({ toast: toastMock }));

import { routeComponent } from "../../test/route-internals";
import { Route } from "./machines";

const MachinesPage = routeComponent(Route);

let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  useQueryMock.mockReset().mockReturnValue([]);
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
});

afterEach(() => {
  toastMock.mockClear();
  toastMock.error.mockClear();
});

describe("MachinesPage empty state", () => {
  it("renders the init command as inline code, never as markdown backticks", () => {
    render(<MachinesPage />);
    const body = screen.getByTestId("machines-empty-body");
    expect(body.textContent).not.toContain("`");
    const code = body.querySelector("code");
    expect(code).not.toBeNull();
    expect(code).toHaveTextContent("nimbus machine init");
  });

  it("gives the empty state a next action that copies the command", async () => {
    render(<MachinesPage />);
    fireEvent.click(screen.getByTestId("machines-empty-cta"));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("nimbus machine init"),
    );
    expect(toastMock).toHaveBeenCalledWith("Copied nimbus machine init");
  });
});
