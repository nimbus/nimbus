import type { Meta, StoryObj } from "@storybook/react";

import { LoadingCell } from "../components/loading-cell";
import type { LoadingValue } from "../shell/loading-value";

const meta: Meta<typeof LoadingCell> = {
  title: "Components/LoadingCell",
  component: LoadingCell,
};

export default meta;

type CellValue = LoadingValue<number>;
type Story = StoryObj<{ value: CellValue }>;

function Render({ value }: { value: CellValue }) {
  return (
    <span className="font-mono text-sm text-default">
      <LoadingCell value={value} testid="story">
        {(n) => <span>{n} rows</span>}
      </LoadingCell>
    </span>
  );
}

export const Ok: Story = {
  render: () => <Render value={{ kind: "ok", value: 1273 }} />,
};

export const Loading: Story = {
  render: () => <Render value={{ kind: "loading" }} />,
};

export const Offline: Story = {
  render: () => <Render value={{ kind: "offline" }} />,
};

export const ErrorState: Story = {
  render: () => (
    <Render value={{ kind: "error", message: "Request failed: 500" }} />
  ),
};

export const InTable: Story = {
  render: () => (
    <table className="text-sm">
      <tbody>
        <tr>
          <td className="px-3 py-2 text-muted">rows</td>
          <td className="px-3 py-2 text-right">
            <LoadingCell value={{ kind: "ok", value: 42 }} testid="story-ok">
              {(n) => <span className="font-mono tabular">{n}</span>}
            </LoadingCell>
          </td>
        </tr>
        <tr>
          <td className="px-3 py-2 text-muted">loading</td>
          <td className="px-3 py-2 text-right">
            <LoadingCell<number>
              value={{ kind: "loading" }}
              testid="story-loading"
            >
              {(n) => <span>{n}</span>}
            </LoadingCell>
          </td>
        </tr>
        <tr>
          <td className="px-3 py-2 text-muted">offline</td>
          <td className="px-3 py-2 text-right">
            <LoadingCell<number>
              value={{ kind: "offline" }}
              testid="story-offline"
            >
              {(n) => <span>{n}</span>}
            </LoadingCell>
          </td>
        </tr>
      </tbody>
    </table>
  ),
};
