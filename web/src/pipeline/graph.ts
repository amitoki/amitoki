import type { Topology } from "../api/types";
import { labels } from "../labels";
export interface GraphNode {
  id: string;
  label: string;
  detail: string;
  kind: "source" | "stage" | "relay" | "inject" | "core";
  depth: number;
}
export interface GraphEdge {
  from: string;
  to: string;
}
export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export function buildGraph(topology: Topology): Graph {
  const nodes = new Map<string, GraphNode>();
  const add = (node: Omit<GraphNode, "depth">) =>
    nodes.set(node.id, { ...node, depth: 0 });
  add({
    id: "capture",
    label: "capture",
    detail: `${topology.interface} · ${labels.required}`,
    kind: "source",
  });
  for (const stage of topology.stages)
    add({ id: stage.id, label: stage.id, detail: stage.plugin, kind: "stage" });
  for (const relay of topology.relays)
    add({ id: relay.id, label: relay.id, detail: relay.plugin, kind: "relay" });
  const edges: GraphEdge[] = [];
  for (const route of topology.routes) {
    let from = route.from;
    if (from.endsWith(".received"))
      add({ id: from, label: from, detail: labels.required, kind: "source" });
    else if (from !== "capture") from = from.split(".")[0];
    for (const to of route.to) {
      if (to === "inject" && !nodes.has(to))
        add({ id: to, label: to, detail: labels.postcheck, kind: "inject" });
      if (!nodes.has(from) || !nodes.has(to)) continue;
      if (nodes.get(to)!.kind !== "relay") {
        edges.push({ from, to });
        continue;
      }
      const check = `check:${to}`;
      if (!nodes.has(check)) {
        add({ id: check, label: labels.postcheck, detail: "", kind: "core" });
        edges.push({ from: check, to });
      }
      edges.push({ from, to: check });
    }
  }
  // 宣言順ではなく、ポートを含む経路のDAGから段数を決める。
  const incoming = new Map(
    [...nodes.keys()].map((id) => [
      id,
      edges.filter((edge) => edge.to === id).length,
    ]),
  );
  const ready = [...nodes.keys()].filter((id) => incoming.get(id) === 0);
  for (let index = 0; index < ready.length; index++) {
    const id = ready[index];
    for (const edge of edges.filter((edge) => edge.from === id)) {
      nodes.get(edge.to)!.depth = Math.max(
        nodes.get(edge.to)!.depth,
        nodes.get(id)!.depth + 1,
      );
      incoming.set(edge.to, incoming.get(edge.to)! - 1);
      if (incoming.get(edge.to) === 0) ready.push(edge.to);
    }
  }
  return { nodes: [...nodes.values()], edges };
}
