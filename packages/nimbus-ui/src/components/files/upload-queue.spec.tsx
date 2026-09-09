import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { type UploadItem, UploadQueue, uploadPercent } from "./upload-queue";

const UPLOADING: UploadItem = {
  id: "a",
  name: "photo.png",
  loaded: 512,
  total: 1024,
  status: "uploading",
};

describe("UploadQueue", () => {
  it("renders nothing without items", () => {
    const { container } = render(
      <UploadQueue items={[]} onDismiss={() => undefined} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows a progress bar and percent while the bytes move", () => {
    render(<UploadQueue items={[UPLOADING]} onDismiss={() => undefined} />);
    expect(screen.getByTestId("upload-queue-progress-a")).toHaveAttribute(
      "value",
      "50",
    );
    expect(screen.getByTestId("upload-queue-percent-a")).toHaveTextContent(
      "50%",
    );
  });

  it("shows the size when done and the reason when refused, both dismissable", () => {
    const onDismiss = vi.fn();
    render(
      <UploadQueue
        items={[
          { ...UPLOADING, id: "b", status: "done", loaded: 1024 },
          { ...UPLOADING, id: "c", status: "error", error: "too large" },
        ]}
        onDismiss={onDismiss}
      />,
    );
    expect(screen.getByTestId("upload-queue-item-b")).toHaveTextContent(
      "1 KiB",
    );
    expect(screen.getByTestId("upload-queue-error-c")).toHaveTextContent(
      "too large",
    );
    fireEvent.click(screen.getByTestId("upload-queue-dismiss-c"));
    expect(onDismiss).toHaveBeenCalledWith("c");
  });
});

describe("uploadPercent", () => {
  it("clamps and floors", () => {
    expect(uploadPercent(UPLOADING)).toBe(50);
    expect(uploadPercent({ ...UPLOADING, loaded: 2048 })).toBe(100);
    expect(uploadPercent({ ...UPLOADING, total: 0 })).toBe(0);
    expect(uploadPercent({ ...UPLOADING, status: "done", loaded: 0 })).toBe(
      100,
    );
  });
});
