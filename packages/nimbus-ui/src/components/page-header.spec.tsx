import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PageHeader } from "./page-header";

describe("PageHeader", () => {
  it("renders the title as a level-1 heading", () => {
    render(<PageHeader title="Nodes" />);
    const heading = screen.getByRole("heading", { level: 1, name: "Nodes" });
    expect(heading.tagName).toBe("H1");
  });

  it("renders the subtitle when provided", () => {
    render(<PageHeader title="Machines" subtitle="Outer dev VMs" />);
    expect(screen.getByText("Outer dev VMs")).toBeInTheDocument();
  });

  // The guard is for prose `subtitle-measure.spec.ts` cannot read statically
  // -- a subtitle built at runtime, say from a server string. Copy inside the
  // 100-character budget never reaches it, so this is the only test that can
  // show it is still wired.
  it("guards against a runaway subtitle", () => {
    const { container } = render(
      <PageHeader
        title="Services"
        subtitle="Long-running processes with their own lifecycle: they start with the server, restart on failure, and expose health over the local socket."
      />,
    );
    expect(container.querySelector("p")).toHaveClass("max-w-[110ch]");
  });

  it("omits the subtitle paragraph when none is given", () => {
    const { container } = render(<PageHeader title="Network" />);
    expect(container.querySelector("p")).toBeNull();
  });

  it("renders the trailing slot only when provided", () => {
    const { rerender, container } = render(<PageHeader title="X" />);
    expect(container.querySelectorAll("header > div").length).toBe(1);
    rerender(
      <PageHeader
        title="X"
        trailing={<span data-testid="trailing">42</span>}
      />,
    );
    expect(screen.getByTestId("trailing")).toHaveTextContent("42");
  });

  it("forwards the test id to the header element", () => {
    render(<PageHeader title="X" testid="page-x-header" />);
    expect(screen.getByTestId("page-x-header").tagName).toBe("HEADER");
  });
});
