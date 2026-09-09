import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { IndexesTab } from "./indexes-tab";

describe("IndexesTab", () => {
  it("says when the schema declares no indexes", () => {
    render(<IndexesTab schema={null} />);
    expect(screen.getByTestId("documents-indexes-empty")).toHaveTextContent(
      "No indexes defined",
    );
    expect(
      screen.queryByTestId("documents-indexes-table"),
    ).not.toBeInTheDocument();
  });

  it("lists each index with its fields and uniqueness", () => {
    render(
      <IndexesTab
        schema={{
          table: "users",
          fields: [],
          indexes: [
            { name: "by_email", fields: ["email"], unique: true },
            { name: "by_org_role", fields: ["org", "role"] },
          ],
        }}
      />,
    );
    const table = screen.getByTestId("documents-indexes-table");
    expect(table).toHaveAttribute("aria-rowcount", "3");
    const rows = screen.getAllByTestId("documents-indexes-table-row");
    expect(rows[0]).toHaveTextContent("by_email");
    expect(rows[0]).toHaveTextContent("email");
    expect(rows[0]).toHaveTextContent("yes");
    expect(rows[1]).toHaveTextContent("org, role");
    expect(rows[1]).toHaveTextContent("no");
  });

  // The index API has no create or drop endpoint yet. The tab says so
  // instead of showing controls that cannot work.
  it("says the view is read-only", () => {
    render(<IndexesTab schema={null} />);
    expect(screen.getByTestId("documents-indexes-tab")).toHaveTextContent(
      "Read-only",
    );
  });
});
