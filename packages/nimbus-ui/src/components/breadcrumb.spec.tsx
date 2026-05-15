import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({
    to,
    children,
    ...rest
  }: {
    to: string;
    children: React.ReactNode;
  } & React.AnchorHTMLAttributes<HTMLAnchorElement>) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

import { Breadcrumb } from "./breadcrumb";

describe("Breadcrumb", () => {
  it("renders the nav landmark with a clear aria-label", () => {
    render(<Breadcrumb segments={[{ label: "Storage" }]} />);
    expect(
      screen.getByRole("navigation", { name: /resource breadcrumb/i }),
    ).toBeInTheDocument();
  });

  it("renders intermediate segments as links and the active one as a span", () => {
    render(
      <Breadcrumb
        segments={[
          { label: "Storage", href: "/storage" },
          { label: "demo", href: "/storage/demo" },
          { label: "machines", active: true },
        ]}
      />,
    );
    expect(screen.getByTestId("breadcrumb-link-0")).toHaveAttribute(
      "href",
      "/storage",
    );
    expect(screen.getByTestId("breadcrumb-link-1")).toHaveAttribute(
      "href",
      "/storage/demo",
    );
    expect(screen.getByTestId("breadcrumb-segment-2")).toHaveTextContent(
      "machines",
    );
  });

  it("renders the chevron only between segments", () => {
    const { container } = render(
      <Breadcrumb
        segments={[
          { label: "Storage", href: "/storage" },
          { label: "demo", active: true },
        ]}
      />,
    );
    const chevrons = container.querySelectorAll("[aria-hidden='true']");
    expect(chevrons).toHaveLength(1);
  });

  it("renders a CopyChip when copyValue is provided", () => {
    render(
      <Breadcrumb
        segments={[
          { label: "demo", copyValue: "tnt_demo", copyLabel: "tenant id" },
        ]}
      />,
    );
    const chip = screen.getByTestId("breadcrumb-copy-0");
    expect(chip).toHaveAttribute("aria-label", "Copy tenant id: tnt_demo");
  });
});
