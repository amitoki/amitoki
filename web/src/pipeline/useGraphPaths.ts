import { useLayoutEffect, useRef, useState } from "react";
import type { Graph } from "./graph";

interface Drawing {
  viewBox: string;
  paths: string[];
}
export function useGraphPaths(graph: Graph) {
  const container = useRef<HTMLDivElement>(null);
  const [drawing, setDrawing] = useState<Drawing>({
    viewBox: "0 0 1 1",
    paths: [],
  });
  useLayoutEffect(() => {
    const element = container.current;
    if (!element) return;
    function draw() {
      if (!element) return;
      const bounds = element.getBoundingClientRect();
      const elements = new Map(
        [...element.querySelectorAll<HTMLElement>("[data-node]")].map(
          (node) => [node.dataset.node, node],
        ),
      );
      const vertical = getComputedStyle(element).flexDirection === "column";
      const paths: string[] = [];
      for (const edge of graph.edges) {
        const from = elements.get(edge.from)?.getBoundingClientRect();
        const to = elements.get(edge.to)?.getBoundingClientRect();
        if (!from || !to) continue;
        const start = vertical
          ? [from.left + from.width / 2 - bounds.left, from.bottom - bounds.top]
          : [from.right - bounds.left, from.top + from.height / 2 - bounds.top];
        const end = vertical
          ? [to.left + to.width / 2 - bounds.left, to.top - bounds.top]
          : [to.left - bounds.left, to.top + to.height / 2 - bounds.top];
        const middle = vertical
          ? (start[1] + end[1]) / 2
          : (start[0] + end[0]) / 2;
        paths.push(
          vertical
            ? `M${start} C${start[0]},${middle} ${end[0]},${middle} ${end}`
            : `M${start} C${middle},${start[1]} ${middle},${end[1]} ${end}`,
        );
        paths.push(
          vertical
            ? `M${end[0] - 3},${end[1] - 5} L${end} L${end[0] + 3},${end[1] - 5}`
            : `M${end[0] - 5},${end[1] - 3} L${end} L${end[0] - 5},${end[1] + 3}`,
        );
      }
      setDrawing({ viewBox: `0 0 ${bounds.width} ${bounds.height}`, paths });
    }
    const observer = new ResizeObserver(draw);
    observer.observe(element);
    draw();
    return () => observer.disconnect();
  }, [graph]);
  return { container, drawing };
}
