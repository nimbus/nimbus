import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { shortId } from "../../lib/format";
import { CellValue } from "./cell-value";

describe("CellValue", () => {
  it("renders the _id column as a copyable short-id chip", () => {
    render(<CellValue value={undefined} field="_id" id="doc_abcdef123456" />);
    const chip = screen.getByTestId("documents-cell-id-doc_abcdef123456");
    expect(chip).toBeInTheDocument();
    expect(chip).toHaveTextContent(shortId("doc_abcdef123456"));
  });

  it("renders a dash for null and undefined values", () => {
    const { rerender } = render(
      <CellValue value={null} field="name" id="x" />,
    );
    expect(screen.getByText("—")).toBeInTheDocument();
    rerender(<CellValue value={undefined} field="name" id="x" />);
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("renders a string inline with the full value as its title", () => {
    render(<CellValue value="hello world" field="name" id="x" />);
    const cell = screen.getByText("hello world");
    expect(cell).toHaveAttribute("title", "hello world");
  });

  it("renders numbers and booleans as text", () => {
    const { rerender } = render(<CellValue value={42} field="n" id="x" />);
    expect(screen.getByText("42")).toBeInTheDocument();
    rerender(<CellValue value={true} field="b" id="x" />);
    expect(screen.getByText("true")).toBeInTheDocument();
  });

  it("falls back to a truncated JSON preview for objects", () => {
    render(<CellValue value={{ a: 1 }} field="data" id="x" />);
    const cell = screen.getByText('{"a":1}');
    expect(cell).toBeInTheDocument();
    expect(cell).toHaveAttribute("title", '{"a":1}');
  });
});
