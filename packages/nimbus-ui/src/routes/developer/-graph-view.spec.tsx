import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const navigateMock = vi.fn();
const buildLocationMock = vi.fn(
  ({ params }: { params: { function: string } }) => ({
    href: `/ui/developer/compute/${params.function}?tab=source`,
  }),
);

vi.mock("@tanstack/react-router", () => ({
  useRouter: () => ({
    buildLocation: buildLocationMock,
    navigate: navigateMock,
  }),
}));

vi.mock("../../hooks/use-api-read", () => ({
  useApiRead: () => ({
    kind: "ok",
    value: {
      nodes: [{ id: "messages:send", module: "messages", name: "send" }],
      edges: [],
    },
  }),
}));

import { GraphView } from "./-graph-view";

describe("GraphView", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    buildLocationMock.mockClear();
  });

  it("renders each function as a keyboard-accessible link", () => {
    render(<GraphView />);

    const link = screen.getByRole("link", { name: "Open send" });
    expect(link).toHaveAttribute(
      "href",
      "/ui/developer/compute/messages:send?tab=source",
    );
  });

  it("uses client navigation for an unmodified click", () => {
    render(<GraphView />);

    fireEvent.click(screen.getByRole("link", { name: "Open send" }));
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/developer/compute/$function",
      params: { function: "messages:send" },
      search: { tab: "source" },
    });
  });

  it("keeps modified clicks available to the browser", () => {
    render(<GraphView />);

    fireEvent.click(screen.getByRole("link", { name: "Open send" }), {
      metaKey: true,
    });
    expect(navigateMock).not.toHaveBeenCalled();
  });
});
