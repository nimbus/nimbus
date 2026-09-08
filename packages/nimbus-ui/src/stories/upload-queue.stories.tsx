import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { DropZone } from "../components/files/drop-zone";
import { type UploadItem, UploadQueue } from "../components/files/upload-queue";

const meta: Meta<typeof UploadQueue> = {
  title: "Components/UploadQueue",
  component: UploadQueue,
};

export default meta;

type Story = StoryObj<typeof UploadQueue>;

const ITEMS: UploadItem[] = [
  {
    id: "1",
    name: "hero.png",
    loaded: 640_000,
    total: 1_200_000,
    status: "uploading",
  },
  {
    id: "2",
    name: "notes/readme.md",
    loaded: 2_048,
    total: 2_048,
    status: "done",
  },
  {
    id: "3",
    name: "backup.tar",
    loaded: 0,
    total: 40_000_000,
    status: "error",
    error: "Object exceeds the 16 MiB whole-object limit",
  },
];

export const Mixed: Story = {
  render: () => {
    const [items, setItems] = useState(ITEMS);
    return (
      <div className="w-[560px]">
        <UploadQueue
          items={items}
          onDismiss={(id) =>
            setItems((prev) => prev.filter((i) => i.id !== id))
          }
        />
      </div>
    );
  },
};

export const WithDropZone: Story = {
  render: () => {
    const [items, setItems] = useState<UploadItem[]>([]);
    return (
      <div className="flex w-[560px] flex-col gap-3">
        <DropZone
          onFiles={(files) =>
            setItems((prev) => [
              ...prev,
              ...files.map((file, index) => ({
                id: `${Date.now()}-${index}`,
                name: file.name,
                loaded: file.size,
                total: file.size,
                status: "done" as const,
              })),
            ])
          }
        >
          <div className="flex h-40 items-center justify-center rounded-md border border-border-2 bg-bg-panel text-sm text-text-3">
            Drag a file over this area
          </div>
        </DropZone>
        <UploadQueue
          items={items}
          onDismiss={(id) =>
            setItems((prev) => prev.filter((i) => i.id !== id))
          }
        />
      </div>
    );
  },
};
