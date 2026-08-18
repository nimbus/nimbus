import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/developer/does-not-exist" },
}));

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
  useRouterState: ({
    select,
  }: {
    select: (state: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

import { NotFound } from "./not-found";

beforeEach(() => {
  pathnameRef.current = "/developer/does-not-exist";
});

describe("NotFound", () => {
  it("names the attempted pathname in a code element", () => {
    render(<NotFound />);
    const body = screen.getByTestId("route-not-found-body");
    const code = body.querySelector("code");
    expect(code).not.toBeNull();
    expect(code).toHaveTextContent("/developer/does-not-exist");
  });

  it("renders prose without markdown backticks", () => {
    render(<NotFound />);
    expect(
      screen.getByTestId("route-not-found-body").textContent,
    ).not.toContain("`");
    expect(
      screen.getByTestId("route-not-found-title").textContent,
    ).not.toContain("`");
  });

  it("offers a way back to the developer view root for a developer path", () => {
    render(<NotFound />);
    const cta = screen.getByTestId("route-not-found-cta");
    expect(cta).toHaveAttribute("href", "/developer");
    expect(cta).toHaveTextContent("Go to Overview");
  });

  it("offers a way back to the operator view root for an operator path", () => {
    pathnameRef.current = "/operator/nope/deep";
    render(<NotFound />);
    const cta = screen.getByTestId("route-not-found-cta");
    expect(cta).toHaveAttribute("href", "/operator");
    expect(cta).toHaveTextContent("Go to Nodes");
  });
});
