import { render, screen } from "@testing-library/react";
import { Inbox } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";

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
    expect(screen.queryByTestId("es-snippet")).toBeNull();
  });

  it("renders the title in the sans face: it is a sentence, not data", () => {
    render(<EmptyState title="Tenants endpoint unavailable" testid="es" />);
    expect(screen.getByTestId("es-title")).not.toHaveClass("font-mono");
    expect(screen.getByTestId("es-title")).toHaveClass("text-base");
  });

  it("renders body content and the icon when provided", () => {
    const { container } = render(
      <EmptyState
        icon={Inbox}
        title="Files"
        body="Buckets and uploads will live here."
        testid="es"
      />,
    );
    expect(screen.getByTestId("es-body")).toHaveTextContent(
      "Buckets and uploads will live here.",
    );
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("puts the mascot in the icon's place for the shell's own screens", () => {
    render(
      <EmptyState mascot="error" title="The console shell failed to render" />,
    );
    const mascot = document.querySelector("svg[data-state]");
    expect(mascot).toHaveAttribute("data-state", "error");
    expect(mascot).toHaveAttribute("aria-hidden", "true");
    expect(mascot?.getAttribute("width")).toBe("56");
  });

  it("renders a Link cta when given to:", () => {
    render(
      <EmptyState
        title="Files"
        cta={{ label: "View settings", to: "/operator/settings" }}
        testid="es"
      />,
    );
    const cta = screen.getByTestId("es-cta");
    expect(cta.tagName).toBe("A");
    expect(cta).toHaveAttribute("href", "/operator/settings");
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

  it("renders a multi-line command as a block, whole, with one copy control", () => {
    const snippet =
      'curl -s -X POST http://localhost:3210/api/tenants \\\n  -d \'{"id": "demo"}\'';
    render(
      <TooltipProvider>
        <EmptyState title="No tenants" snippet={snippet} testid="es" />
      </TooltipProvider>,
    );
    const block = screen.getByTestId("es-snippet");
    expect(block.querySelector("pre")).not.toBeNull();
    expect(block.querySelector("code")).toHaveClass("font-mono");
    expect(block.querySelector("code")?.textContent).toBe(snippet);
    expect(
      screen.getAllByRole("button", { name: "Copy command" }),
    ).toHaveLength(1);
  });

  it("renders the one command in mono with a copy control", () => {
    render(
      <TooltipProvider>
        <EmptyState title="No functions" snippet="nimbus deploy" testid="es" />
      </TooltipProvider>,
    );
    const snippet = screen.getByTestId("es-snippet");
    expect(snippet.querySelector("code")).toHaveClass("font-mono");
    expect(snippet).toHaveTextContent("nimbus deploy");
    expect(
      screen.getByRole("button", { name: "Copy command" }),
    ).toBeInTheDocument();
  });
});
