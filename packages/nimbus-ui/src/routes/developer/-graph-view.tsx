import { useRouter } from "@tanstack/react-router";
import { useMemo } from "react";

import { useApiRead } from "../../hooks/use-api-read";

// Deployment call graph (FSV7): functions as nodes, api.*/internal.* calls as
// edges, laid out in columns by module. Data from GET /api/console/graph.

type GraphNode = { id: string; module: string; name: string };
type GraphEdge = { from: string; to: string };
type GraphData = { nodes: GraphNode[]; edges: GraphEdge[] };

const NODE_W = 168;
const NODE_H = 30;
const NODE_GAP_Y = 14;
const COL_GAP_X = 96;
const HEADER_H = 26;
const PAD = 28;
const COL_W = NODE_W + COL_GAP_X;

export function GraphView() {
  const state = useApiRead<GraphData>("/api/console/graph");

  return (
    <div
      className="min-h-0 flex-1 overflow-auto rounded-md border border-app bg-surface"
      data-testid="compute-graph"
    >
      {state.kind === "ok" ? (
        state.value.nodes.length === 0 ? (
          <Centered>
            No functions deployed yet. Deploy an app to see its call graph.
          </Centered>
        ) : (
          <GraphCanvas graph={state.value} />
        )
      ) : state.kind === "loading" ? (
        <Centered>Loading call graph…</Centered>
      ) : (
        <Centered>
          Could not load the call graph (
          {state.kind === "error" ? state.message : "offline"}).
        </Centered>
      )}
    </div>
  );
}

function GraphCanvas({ graph }: { graph: GraphData }) {
  const router = useRouter();

  const layout = useMemo(() => {
    const modules = Array.from(
      new Set(graph.nodes.map((n) => n.module)),
    ).sort();
    const byModule = new Map<string, GraphNode[]>();
    for (const module of modules) byModule.set(module, []);
    for (const node of [...graph.nodes].sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      byModule.get(node.module)?.push(node);
    }
    const pos = new Map<string, { x: number; y: number }>();
    modules.forEach((module, mi) => {
      const colX = PAD + mi * COL_W;
      byModule.get(module)?.forEach((node, ni) => {
        pos.set(node.id, {
          x: colX,
          y: PAD + HEADER_H + ni * (NODE_H + NODE_GAP_Y),
        });
      });
    });
    const maxRows = Math.max(
      1,
      ...modules.map((m) => byModule.get(m)?.length ?? 0),
    );
    const width = PAD * 2 + modules.length * COL_W - COL_GAP_X;
    const height = PAD * 2 + HEADER_H + maxRows * (NODE_H + NODE_GAP_Y);
    return { modules, byModule, pos, width, height };
  }, [graph]);

  const functionLocation = (id: string) =>
    router.buildLocation({
      to: "/developer/compute/$function",
      params: { function: id },
      search: { tab: "source" },
    });

  const openFunction = (id: string) =>
    router.navigate({
      to: "/developer/compute/$function",
      params: { function: id },
      search: { tab: "source" },
    });

  return (
    <svg
      width={layout.width}
      height={layout.height}
      viewBox={`0 0 ${layout.width} ${layout.height}`}
      className="min-w-full"
      role="img"
      aria-label="Function call graph"
    >
      <title>Function call graph</title>
      <defs>
        <marker
          id="nimbus-graph-arrow"
          viewBox="0 0 10 10"
          refX="9"
          refY="5"
          markerWidth="6"
          markerHeight="6"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" fill="var(--nimbus-muted)" />
        </marker>
      </defs>

      {/* module column headers */}
      {layout.modules.map((module, mi) => (
        <text
          key={module}
          x={PAD + mi * COL_W}
          y={PAD}
          className="font-mono"
          fontSize="10"
          letterSpacing="1.4"
          fill="var(--nimbus-muted)"
          style={{ textTransform: "uppercase" }}
        >
          {module}
        </text>
      ))}

      {/* edges */}
      {graph.edges.map((edge) => {
        const from = layout.pos.get(edge.from);
        const to = layout.pos.get(edge.to);
        if (!from || !to) return null;
        const x1 = from.x + NODE_W;
        const y1 = from.y + NODE_H / 2;
        const x2 = to.x;
        const y2 = to.y + NODE_H / 2;
        const dx = Math.max(40, Math.abs(x2 - x1) / 2);
        return (
          <path
            key={`${edge.from}->${edge.to}`}
            d={`M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`}
            fill="none"
            stroke="var(--nimbus-muted)"
            strokeWidth="1.5"
            strokeOpacity="0.7"
            markerEnd="url(#nimbus-graph-arrow)"
          />
        );
      })}

      {/* nodes */}
      {graph.nodes.map((node) => {
        const p = layout.pos.get(node.id);
        if (!p) return null;
        return (
          <g key={node.id} transform={`translate(${p.x}, ${p.y})`}>
            <a
              href={functionLocation(node.id).href}
              data-testid={`graph-node-${node.id}`}
              aria-label={`Open ${node.name}`}
              onClick={(event) => {
                if (
                  event.button !== 0 ||
                  event.metaKey ||
                  event.ctrlKey ||
                  event.shiftKey ||
                  event.altKey
                ) {
                  return;
                }
                event.preventDefault();
                void openFunction(node.id);
              }}
              style={{ cursor: "pointer" }}
            >
              <rect
                width={NODE_W}
                height={NODE_H}
                rx="6"
                fill="var(--nimbus-surface-2)"
                stroke="var(--nimbus-border-strong)"
              />
              <text
                x={10}
                y={NODE_H / 2 + 4}
                className="font-mono"
                fontSize="12"
                fill="var(--nimbus-text)"
              >
                {node.name}
              </text>
            </a>
          </g>
        );
      })}
    </svg>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-40 items-center justify-center px-6 text-center text-xs text-muted">
      {children}
    </div>
  );
}
