import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { invalidateMock } = vi.hoisted(() => ({ invalidateMock: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useRouter: () => ({ invalidate: invalidateMock }),
}));

import {
  AdminServiceDetailLoaderError,
  AdminServicesLoaderError,
  ServiceDetailLoaderError,
  ServicesLoaderError,
} from "./service-loader-errors";

beforeEach(() => {
  invalidateMock.mockReset();
});

describe("developer loader errors", () => {
  it("ServicesLoaderError shows the message and retries through reset", async () => {
    const user = userEvent.setup();
    const reset = vi.fn();
    render(
      <ServicesLoaderError error={new Error("convex down")} reset={reset} />,
    );
    expect(screen.getByTestId("page-services")).toBeInTheDocument();
    expect(
      screen.getByText("Services endpoint unavailable"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("storage-server-error")).toHaveTextContent(
      "convex down",
    );
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it("ServiceDetailLoaderError stringifies a non-Error rejection", () => {
    render(<ServiceDetailLoaderError error="socket closed" reset={vi.fn()} />);
    expect(screen.getByTestId("page-service-detail")).toBeInTheDocument();
    expect(screen.getByTestId("storage-server-error")).toHaveTextContent(
      "socket closed",
    );
  });
});

describe("operator loader errors", () => {
  it("AdminServicesLoaderError retries by invalidating the router", async () => {
    const user = userEvent.setup();
    render(<AdminServicesLoaderError error={new Error("down")} />);
    expect(screen.getByTestId("page-admin-services")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(invalidateMock).toHaveBeenCalledTimes(1);
  });

  it("AdminServiceDetailLoaderError retries by invalidating the router", async () => {
    const user = userEvent.setup();
    render(<AdminServiceDetailLoaderError error={new Error("down")} />);
    expect(screen.getByTestId("page-admin-service-detail")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(invalidateMock).toHaveBeenCalledTimes(1);
  });
});
