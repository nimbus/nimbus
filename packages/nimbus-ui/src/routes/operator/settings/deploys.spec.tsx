import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DeploysSection } from "./deploys";

describe("DeploysSection copy", () => {
  // The section description was authored as markdown and dropped into a text
  // node, so the user read a literal ` around the command. Commands are marked
  // up as <code>.
  it("marks the deploy command up as <code> and leaks no backticks", () => {
    render(<DeploysSection bundles={[]} functions={[]} />);

    const section = screen.getByTestId("settings-deploys");
    expect(section.textContent).not.toContain("`");

    const description = section.querySelector("header p");
    expect(description?.textContent).toContain(
      "Trigger new deploys with nimbus deploy from the CLI.",
    );

    const command = description?.querySelector("code");
    expect(command?.textContent).toBe("nimbus deploy");
    // A wrapped multi-word command renders as two separate boxed fragments.
    expect(command?.className).toContain("whitespace-nowrap");
  });

  it("leaks no backticks in the no-bundles empty state", () => {
    render(<DeploysSection bundles={[]} functions={[]} />);

    const empty = screen.getByTestId("settings-deploys-empty");
    expect(empty.textContent).not.toContain("`");
    expect(empty.querySelector("code")?.textContent).toBe("nimbus deploy");
  });
});
