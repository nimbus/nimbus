import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { formatAbsoluteTime, shortId } from "../../lib/format";
import { CellValue } from "./cell-value";

describe("CellValue", () => {
  it("renders the _id column as a copyable short-id chip", () => {
    render(<CellValue value={undefined} field="_id" id="doc_abcdef123456" />);
    const chip = screen.getByTestId("documents-cell-id-doc_abcdef123456");
    expect(chip).toBeInTheDocument();
    expect(chip).toHaveTextContent(shortId("doc_abcdef123456"));
  });

  // Absent, null and empty-string are three different facts about a document.
  // Collapsing them into one glyph makes a schema question unanswerable from
  // the browser, so each one has to stay distinguishable on screen.
  it("distinguishes an absent field from a null value", () => {
    const { rerender } = render(
      <CellValue value={undefined} field="name" id="x" />,
    );
    const absent = screen.getByText("—");
    expect(absent).toHaveAttribute(
      "title",
      "field not present in this document",
    );
    expect(screen.queryByText("null")).not.toBeInTheDocument();

    rerender(<CellValue value={null} field="name" id="x" />);
    expect(screen.getByText("null")).toBeInTheDocument();
    expect(screen.queryByText("—")).not.toBeInTheDocument();
  });

  it("renders an empty string as a quoted empty value, not a dash", () => {
    render(<CellValue value="" field="name" id="x" />);
    expect(screen.getByTitle("empty string")).toHaveTextContent('""');
    expect(screen.queryByText("—")).not.toBeInTheDocument();
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

  it("renders an epoch-ms timestamp field as an absolute time, keeping the raw number in the title", () => {
    const epoch = 1_752_000_000_000;
    render(<CellValue value={epoch} field="_creationTime" id="x" />);
    const cell = screen.getByText(formatAbsoluteTime(epoch));
    expect(cell).toHaveAttribute("title", String(epoch));
  });

  // A JSON prefix cut at a fixed width made an object and an array look alike
  // and gave no signal that anything had been removed. The chip states the
  // shape and the size instead, and carries the full value in its title.
  it("summarises containers by shape and size instead of previewing JSON", () => {
    const { rerender } = render(
      <CellValue value={{ a: 1, b: 2 }} field="data" id="x" />,
    );
    const object = screen.getByText("{…} 2 keys");
    expect(object).toHaveAttribute(
      "title",
      JSON.stringify({ a: 1, b: 2 }, null, 2),
    );

    rerender(<CellValue value={[1, 2, 3]} field="data" id="x" />);
    expect(screen.getByText("[…] 3 items")).toBeInTheDocument();

    rerender(<CellValue value={[]} field="data" id="x" />);
    expect(screen.getByText("[]")).toBeInTheDocument();

    rerender(<CellValue value={{}} field="data" id="x" />);
    expect(screen.getByText("{}")).toBeInTheDocument();
  });

  it("makes a container chip a button that opens the document when it can expand", async () => {
    const onExpand = vi.fn();
    const user = userEvent.setup();
    render(
      <CellValue value={{ a: 1 }} field="data" id="x" onExpand={onExpand} />,
    );
    await user.click(screen.getByRole("button", { name: /1 key/ }));
    expect(onExpand).toHaveBeenCalledTimes(1);
  });

  it("renders a container as inert text when there is nothing to expand into", () => {
    render(<CellValue value={{ a: 1 }} field="data" id="x" />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("{…} 1 key")).toBeInTheDocument();
  });
});
