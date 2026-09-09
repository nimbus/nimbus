import type { Meta, StoryObj } from "@storybook/react";
import type { RowSelectionState, SortingState } from "@tanstack/react-table";
import { useState } from "react";

import {
  DataTable,
  DataTableFooter,
  dataColumns,
  selectionColumn,
  VIRTUAL_THRESHOLD,
} from "../components/data-table";
import { StatePill } from "../components/pill";

type Machine = {
  id: string;
  name: string;
  region: string;
  state: string;
  cpu: number;
  memoryMb: number;
};

const STATES = ["ready", "starting", "degraded", "stopped", "error"];
const REGIONS = ["iad", "sfo", "ams", "sin"];

function machines(count: number): Machine[] {
  return Array.from({ length: count }, (_, i) => ({
    id: `m-${String(i).padStart(4, "0")}`,
    name: `web-${REGIONS[i % REGIONS.length]}-${String(i).padStart(3, "0")}`,
    region: REGIONS[i % REGIONS.length] ?? "iad",
    state: STATES[(i * 3) % STATES.length] ?? "ready",
    cpu: (i * 37) % 100,
    memoryMb: 512 * (1 + (i % 6)),
  }));
}

const col = dataColumns<Machine>();
const COLUMNS = [
  col.accessor("name", {
    header: "Name",
    cell: (ctx) => <span className="font-mono">{ctx.getValue()}</span>,
  }),
  col.accessor("state", {
    header: "State",
    cell: (ctx) => <StatePill state={ctx.getValue()} />,
  }),
  col.accessor("region", { header: "Region" }),
  col.accessor("cpu", {
    header: "CPU %",
    cell: (ctx) => (
      <span className="block text-right font-mono tabular-nums">
        {ctx.getValue()}
      </span>
    ),
  }),
  col.accessor("memoryMb", {
    header: "Memory",
    cell: (ctx) => (
      <span className="block text-right font-mono tabular-nums">
        {ctx.getValue()} MB
      </span>
    ),
  }),
];

const meta: Meta<typeof DataTable<Machine>> = {
  title: "Components/DataTable",
  component: DataTable,
  parameters: { layout: "padded" },
  args: {
    columns: COLUMNS,
    data: machines(8),
    getRowId: (m: Machine) => m.id,
    ariaLabel: "Machines",
  },
};

export default meta;

type Story = StoryObj<typeof DataTable<Machine>>;

export const Basic: Story = {};

export const Empty: Story = {
  args: { data: [], emptyMessage: "No machines match this filter." },
};

// Sorting is controlled: the page owns the state so it can put it in the URL.
export const Sorting: Story = {
  render: (args) => {
    const [sorting, setSorting] = useState<SortingState>([
      { id: "cpu", desc: true },
    ]);
    return (
      <DataTable {...args} sorting={sorting} onSortingChange={setSorting} />
    );
  },
};

export const Selection: Story = {
  render: (args) => {
    const [rowSelection, setRowSelection] = useState<RowSelectionState>({
      "m-0001": true,
    });
    const count = Object.keys(rowSelection).length;
    return (
      <div className="flex flex-col gap-2">
        <span className="text-xs text-text-3">{count} selected</span>
        <DataTable
          {...args}
          columns={[selectionColumn<Machine>((m) => m.name), ...COLUMNS]}
          rowSelection={rowSelection}
          onRowSelectionChange={setRowSelection}
        />
      </div>
    );
  },
};

export const RowActivate: Story = {
  render: (args) => {
    const [active, setActive] = useState<string | null>(null);
    return (
      <div className="flex flex-col gap-2">
        <span className="text-xs text-text-3">
          {active ? `Opened ${active}` : "Click or press Enter on a row"}
        </span>
        <DataTable {...args} onRowActivate={(m) => setActive(m.name)} />
      </div>
    );
  },
};

// Past VIRTUAL_THRESHOLD rows the body virtualizes inside a bounded scroller.
export const Virtualized: Story = {
  args: { data: machines(VIRTUAL_THRESHOLD * 5), maxHeight: 420 },
};

export const WithFooter: Story = {
  render: (args) => (
    <div className="flex flex-col">
      <DataTable {...args} data={machines(50)} maxHeight={320} />
      <DataTableFooter
        loaded={50}
        pageSize={50}
        hasMore
        noun={{ one: "machine", many: "machines" }}
      />
    </div>
  ),
};
