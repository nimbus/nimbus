import { areaY, barY, defineChart, lineY } from "@tanstack/charts";
import { Chart } from "@tanstack/charts/react";
import { scaleBand } from "@tanstack/charts/scales/band";
import { scaleLinear } from "@tanstack/charts/scales/linear";
import { scalePoint } from "@tanstack/charts/scales/point";
import { useMemo } from "react";

import { cn } from "@/lib/utils";

// This file is the only importer of @tanstack/charts. Pages compose the
// three shapes below and never touch the library, so the chart vocabulary
// (colours, axes, grid, motion) is decided once, here.
//
// Charts follow the same rule as text: mono is the voice of data, and the
// one amber accent is never a series colour. A series is a neutral or a
// semantic colour, and the caller picks it through `color`.

export const CHART = {
  neutral: "var(--text-3)",
  neutralStrong: "var(--text-2)",
  neutralSoft: "var(--text-4)",
  grid: "var(--border-1)",
  axis: "var(--text-4)",
  success: "var(--success)",
  warning: "var(--warning)",
  error: "var(--error)",
  info: "var(--info)",
} as const;

const THEME = {
  foreground: CHART.axis,
  muted: CHART.axis,
  grid: CHART.grid,
  background: "transparent",
  palette: [CHART.neutral, CHART.neutralStrong, CHART.neutralSoft],
} as const;

export type ChartPoint = { label: string; value: number };

// Sparkline is a trend with no axes: an area under a line, sized for a
// stat card. It never has a tooltip; the stat next to it is the number.
export function Sparkline({
  data,
  color = CHART.neutral,
  ariaLabel,
  height = 32,
  className,
}: {
  data: ReadonlyArray<ChartPoint>;
  color?: string;
  ariaLabel: string;
  height?: number;
  className?: string;
}) {
  const definition = useMemo(
    () =>
      defineChart({
        marks: [
          areaY(data, {
            x: "label",
            y: "value",
            fill: color,
            fillOpacity: 0.12,
          }),
          lineY(data, {
            x: "label",
            y: "value",
            stroke: color,
            strokeWidth: 1.5,
          }),
        ],
        scales: {
          x: { scale: () => scalePoint<string>(), axis: false },
          y: { scale: scaleLinear, axis: false },
        },
        guides: false,
        margin: 1,
        theme: THEME,
        tooltip: false,
        svgAnimation: false,
        pointer: false,
        keyboard: false,
        focusRing: false,
      }),
    [data, color],
  );
  return (
    <Chart
      definition={definition}
      ariaLabel={ariaLabel}
      height={height}
      className={cn("w-full", className)}
    />
  );
}

// AreaChart is a series over time with a grid and both axes, for a panel
// that reads one metric across a window.
export function AreaChart({
  data,
  color = CHART.neutral,
  ariaLabel,
  height = 160,
  className,
}: {
  data: ReadonlyArray<ChartPoint>;
  color?: string;
  ariaLabel: string;
  height?: number;
  className?: string;
}) {
  const definition = useMemo(
    () =>
      defineChart({
        marks: [
          areaY(data, {
            x: "label",
            y: "value",
            fill: color,
            fillOpacity: 0.12,
          }),
          lineY(data, {
            x: "label",
            y: "value",
            stroke: color,
            strokeWidth: 1.5,
          }),
        ],
        scales: {
          x: { scale: () => scalePoint<string>(), axis: { line: false } },
          y: {
            scale: scaleLinear,
            nice: true,
            grid: true,
            axis: { line: false },
          },
        },
        margin: { top: 8, right: 8, bottom: 24, left: 36 },
        theme: THEME,
        svgAnimation: false,
      }),
    [data, color],
  );
  return (
    <Chart
      definition={definition}
      ariaLabel={ariaLabel}
      height={height}
      className={cn("w-full font-mono text-xs", className)}
    />
  );
}

// BarChart is a count per category: one bar per label with a grid on the
// value axis. `color` may be a function so a bar can take a semantic colour
// by its own datum (an error bucket paints error).
export function BarChart({
  data,
  color = CHART.neutral,
  ariaLabel,
  height = 160,
  className,
}: {
  data: ReadonlyArray<ChartPoint>;
  color?: string | ((point: ChartPoint) => string);
  ariaLabel: string;
  height?: number;
  className?: string;
}) {
  const definition = useMemo(
    () =>
      defineChart({
        marks: [
          barY(data, {
            x: "label",
            y: "value",
            fill: color,
            radius: 2,
            inset: 1,
            maxThickness: 32,
          }),
        ],
        scales: {
          x: {
            scale: () => scaleBand<string>().padding(0.18),
            axis: { line: false },
          },
          y: {
            scale: scaleLinear,
            nice: true,
            grid: true,
            axis: { line: false },
          },
        },
        margin: { top: 8, right: 8, bottom: 24, left: 36 },
        theme: THEME,
        svgAnimation: false,
      }),
    [data, color],
  );
  return (
    <Chart
      definition={definition}
      ariaLabel={ariaLabel}
      height={height}
      className={cn("w-full font-mono text-xs", className)}
    />
  );
}
