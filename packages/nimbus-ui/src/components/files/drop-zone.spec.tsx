import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { DropZone } from "./drop-zone";

function transfer(files: File[], types = ["Files"]) {
  return { dataTransfer: { types, files, dropEffect: "none" } };
}

describe("DropZone", () => {
  it("raises the overlay for a file drag and hands the dropped files over", () => {
    const onFiles = vi.fn();
    render(
      <DropZone onFiles={onFiles} testid="zone">
        <div>table</div>
      </DropZone>,
    );
    const file = new File(["hi"], "hi.txt", { type: "text/plain" });
    fireEvent.dragEnter(screen.getByTestId("zone"), transfer([file]));
    expect(screen.getByTestId("zone-overlay")).toBeInTheDocument();
    fireEvent.drop(screen.getByTestId("zone"), transfer([file]));
    expect(onFiles).toHaveBeenCalledWith([file]);
    expect(screen.queryByTestId("zone-overlay")).toBeNull();
  });

  it("ignores a drag that carries no files", () => {
    const onFiles = vi.fn();
    render(
      <DropZone onFiles={onFiles} testid="zone">
        <div>table</div>
      </DropZone>,
    );
    fireEvent.dragEnter(
      screen.getByTestId("zone"),
      transfer([], ["text/plain"]),
    );
    expect(screen.queryByTestId("zone-overlay")).toBeNull();
    fireEvent.drop(screen.getByTestId("zone"), transfer([], ["text/plain"]));
    expect(onFiles).not.toHaveBeenCalled();
  });

  it("stays inert while disabled", () => {
    const onFiles = vi.fn();
    render(
      <DropZone onFiles={onFiles} disabled testid="zone">
        <div>table</div>
      </DropZone>,
    );
    const file = new File(["hi"], "hi.txt");
    fireEvent.dragEnter(screen.getByTestId("zone"), transfer([file]));
    expect(screen.queryByTestId("zone-overlay")).toBeNull();
    fireEvent.drop(screen.getByTestId("zone"), transfer([file]));
    expect(onFiles).not.toHaveBeenCalled();
  });
});
