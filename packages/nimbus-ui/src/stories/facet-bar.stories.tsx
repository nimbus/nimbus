import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import {
  FacetBar,
  FacetButton,
  FacetInput,
  FacetToggle,
} from "../components/facet-bar";
import { Select } from "../components/select";

const meta: Meta<typeof FacetBar> = {
  title: "Components/FacetBar",
  component: FacetBar,
};

export default meta;

type Story = StoryObj<typeof FacetBar>;

const LEVELS = [
  { value: "*", label: "all levels" },
  { value: "error", label: "error" },
  { value: "warn", label: "warn" },
  { value: "info", label: "info" },
];

export const Default: Story = {
  render: () => {
    const [level, setLevel] = useState("*");
    const [source, setSource] = useState("");
    const [follow, setFollow] = useState(true);
    return (
      <FacetBar label="Log facets" testid="story-facets">
        <Select
          label="Level"
          value={level}
          options={LEVELS}
          onChange={setLevel}
          testid="story-level"
        />
        <FacetInput
          id="story-source"
          label="Source"
          value={source}
          placeholder="source"
          onChange={setSource}
        />
        <FacetToggle label="Follow" value={follow} onChange={setFollow} />
        <FacetButton onClick={() => {}}>clear</FacetButton>
      </FacetBar>
    );
  },
};

export const WithTrailingCluster: Story = {
  render: () => {
    const [paused, setPaused] = useState(true);
    return (
      <div className="w-[480px] overflow-hidden border border-dashed border-border-2 p-2">
        <FacetBar
          label="Log facets"
          trailing={
            <>
              {paused ? (
                <FacetButton tone="danger" onClick={() => setPaused(false)}>
                  paused · resume
                </FacetButton>
              ) : null}
              <FacetToggle label="Follow" value={false} onChange={() => {}} />
              <FacetToggle
                label="Pause on error"
                value={true}
                onChange={() => {}}
              />
              <FacetButton onClick={() => {}}>clear</FacetButton>
              <FacetButton onClick={() => {}} title="System tenant">
                System ⌘\
              </FacetButton>
            </>
          }
        >
          <FacetInput
            id="story-category"
            label="Category"
            value=""
            placeholder="category"
            onChange={() => {}}
          />
          <FacetInput
            id="story-correlation"
            label="Correlation"
            value=""
            placeholder="run id"
            onChange={() => {}}
          />
        </FacetBar>
      </div>
    );
  },
};
