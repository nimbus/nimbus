import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PIN_L, PIN_R, Td, Th } from "./table-cells";

function Table({ children }: { children: React.ReactNode }) {
  return (
    <table>
      <thead>
        <tr>{children}</tr>
      </thead>
    </table>
  );
}

describe("Th", () => {
  it("aligns left by default and right on request", () => {
    render(
      <Table>
        <Th>Name</Th>
        <Th align="right">Size</Th>
      </Table>,
    );
    expect(screen.getByText("Name").className).toContain("text-left");
    expect(screen.getByText("Size").className).toContain("text-right");
  });

  it("declares the column width on the header only", () => {
    render(
      <Table>
        <Th width="30%">Name</Th>
        <Th>Size</Th>
      </Table>,
    );
    expect(screen.getByText("Name").style.width).toBe("30%");
    expect(screen.getByText("Size").style.width).toBe("");
  });
});

describe("Td", () => {
  it("floors the row at the dense step and never wraps", () => {
    render(
      <table>
        <tbody>
          <tr>
            <Td>value</Td>
          </tr>
        </tbody>
      </table>,
    );
    const cell = screen.getByText("value");
    expect(cell.tagName).toBe("TD");
    expect(cell.className).toContain("h-10");
    expect(cell.className).toContain("whitespace-nowrap");
    expect(cell.className).not.toContain("font-mono");
  });

  it("sets mono tabular figures and right alignment for numbers", () => {
    render(
      <table>
        <tbody>
          <tr>
            <Td mono align="right">
              1,024
            </Td>
          </tr>
        </tbody>
      </table>,
    );
    const cell = screen.getByText("1,024");
    expect(cell.className).toContain("font-mono");
    expect(cell.className).toContain("tabular");
    expect(cell.className).toContain("text-right");
  });
});

describe("pinned columns", () => {
  it("pins against the scroller edge on the row background", () => {
    expect(PIN_L).toContain("sticky");
    expect(PIN_R).toContain("right-0");
    expect(PIN_L).toContain("bg-[var(--row-bg)]");
    expect(PIN_R).toContain("bg-[var(--row-bg)]");
  });
});
