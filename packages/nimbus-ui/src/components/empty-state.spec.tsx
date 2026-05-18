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

import { EmptyState } from "./empty-state";

describe("EmptyState", () => {
  it("renders just a title when body and cta are omitted", () => {
    render(<EmptyState title="Object storage coming soon" testid="es" />);
    expect(screen.getByTestId("es-title")).toHaveTextContent(
      "Object storage coming soon",
    );
    expect(screen.queryByTestId("es-body")).toBeNull();
    expect(screen.queryByTestId("es-cta")).toBeNull();
  });

  it("renders body content when provided", () => {
    render(
      <EmptyState
        title="Files"
        body="Buckets and uploads will live here."
        testid="es"
      />,
    );
    expect(screen.getByTestId("es-body")).toHaveTextContent(
      "Buckets and uploads will live here.",
    );
  });

  it("renders a Link cta when given to:", () => {
    render(
      <EmptyState
        title="Files"
        cta={{ label: "View settings", to: "/admin/settings" }}
        testid="es"
      />,
    );
    const cta = screen.getByTestId("es-cta");
    expect(cta.tagName).toBe("A");
    expect(cta).toHaveAttribute("href", "/admin/settings");
    expect(cta).toHaveTextContent("View settings");
  });

  it("renders a button cta and fires onClick", () => {
    const onClick = vi.fn();
    render(
      <EmptyState
        title="Files"
        cta={{ label: "Retry", onClick }}
        testid="es"
      />,
    );
    const cta = screen.getByTestId("es-cta");
    expect(cta.tagName).toBe("BUTTON");
    cta.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
