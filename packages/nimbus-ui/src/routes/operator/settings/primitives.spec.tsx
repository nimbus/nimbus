import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PageSection } from "./primitives";

describe("PageSection", () => {
  it("separates with a header rule instead of a decorative card", () => {
    render(
      <PageSection title="Server" description="What this node is" testid="s">
        <p>body</p>
      </PageSection>,
    );

    const section = screen.getByTestId("s");
    // DESIGN.md's do-not list: page sections are not put inside decorative
    // cards. Framing every section is what produced cards inside cards on this
    // page, so the unframed form has to stay the default.
    expect(section.className).not.toContain("border ");
    expect(section.className).not.toContain("rounded");
    expect(section.className).not.toContain("p-4");

    const heading = screen.getByRole("heading", { name: "Server" });
    const header = heading.closest("header");
    expect(header?.className).toContain("border-b");
    expect(header?.className).toContain("border-app");
  });

  it("frames only when a section opts in, and tones the rule for danger", () => {
    render(
      <PageSection title="Danger zone" testid="dz" tone="danger" framed>
        <p>body</p>
      </PageSection>,
    );

    const section = screen.getByTestId("dz");
    // The frame is the hazard boundary, not decoration — it survives the
    // flattening on purpose.
    expect(section.className).toContain("rounded-md");
    expect(section.className).toContain("border-danger/40");
    expect(section.className).toContain("p-4");
    expect(
      screen.getByRole("heading", { name: "Danger zone" }).className,
    ).toContain("text-danger");
  });

  it("accepts a node description so commands are marked up, not backticked", () => {
    // `description` was `string`, which forced callers to write markdown that
    // then rendered as literal backticks.
    render(
      <PageSection
        title="Deploys"
        testid="d"
        description={
          <>
            Trigger new deploys with <code>nimbus deploy</code>.
          </>
        }
      >
        <p>body</p>
      </PageSection>,
    );

    const description = screen.getByTestId("d").querySelector("header p");
    expect(description?.textContent).toBe(
      "Trigger new deploys with nimbus deploy.",
    );
    expect(description?.querySelector("code")?.textContent).toBe(
      "nimbus deploy",
    );
  });
});
