import { useMemo } from "react";
import type { Topology } from "../api/types";
import { labels } from "../labels";
import { buildGraph } from "./graph";
import { useGraphPaths } from "./useGraphPaths";
interface Props {
  topology: Topology;
  selected?: string;
  onSelect: (stage: string) => void;
}
export function PipelineDiagram({ topology, selected, onSelect }: Props) {
  const graph = useMemo(() => buildGraph(topology), [topology]);
  const { container, drawing } = useGraphPaths(graph);
  const depths = [...new Set(graph.nodes.map((node) => node.depth))].sort(
    (left, right) => left - right,
  );
  return (
    <div
      id="pipeline-diagram"
      className="am-graph relative mb-5 flex flex-col gap-7 py-1"
      aria-label={labels.pipeline}
      ref={container}
    >
      <svg
        className="am-graph-edges pointer-events-none absolute inset-0 size-full overflow-visible [&_path]:fill-none [&_path]:stroke-line [&_path]:stroke-[1.5]"
        aria-hidden="true"
        viewBox={drawing.viewBox}
      >
        {drawing.paths.map((path, index) => (
          <path key={index} d={path} />
        ))}
      </svg>
      {depths.map((depth) => (
        <div
          key={depth}
          className="am-graph-column flex flex-wrap justify-center gap-3.5"
        >
          {graph.nodes
            .filter((node) => node.depth === depth)
            .map((node) =>
              node.kind === "stage" ? (
                <button
                  key={node.id}
                  className="am-graph-node am-stage"
                  type="button"
                  data-node={node.id}
                  data-stage={node.id}
                  aria-pressed={selected === node.id}
                  onClick={() => onSelect(node.id)}
                >
                  <strong>{node.label}</strong>
                  <small>{node.detail}</small>
                </button>
              ) : (
                <div
                  key={node.id}
                  className="am-graph-node am-relay"
                  data-node={node.id}
                >
                  <strong>{node.label}</strong>
                  <small>{node.detail}</small>
                </div>
              ),
            )}
        </div>
      ))}
    </div>
  );
}
