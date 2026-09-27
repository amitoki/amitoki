import { labels as text } from "./labels.js";

function nodesAndEdges(topology) {
  const nodes = new Map();
  const add = ({ id, label, detail, kind }) =>
    nodes.set(id, { id, label, detail, kind, depth: 0 });
  add({
    id: "capture",
    label: "capture",
    detail: `${topology.interface} · ${text.required}`,
    kind: "source",
  });
  for (const stage of topology.stages)
    add({ id: stage.id, label: stage.id, detail: stage.plugin, kind: "stage" });
  for (const relay of topology.relays)
    add({ id: relay.id, label: relay.id, detail: relay.plugin, kind: "relay" });
  const edges = [];
  for (const route of topology.routes) {
    let from = route.from;
    if (from.endsWith(".received"))
      add({ id: from, label: from, detail: text.required, kind: "source" });
    else if (from !== "capture") from = from.split(".")[0];
    for (const to of route.to) {
      if (to === "inject" && !nodes.has(to))
        add({ id: to, label: to, detail: text.postcheck, kind: "inject" });
      if (nodes.has(from) && nodes.has(to)) {
        if (nodes.get(to).kind === "relay") {
          const check = `check:${to}`;
          if (!nodes.has(check)) {
            add({ id: check, label: text.postcheck, detail: "", kind: "core" });
            edges.push({ from: check, to });
          }
          edges.push({ from, to: check });
        } else edges.push({ from, to });
      }
    }
  }
  // ポートを含む経路のDAGから段数を決める。宣言の順序は処理順とみなさない。
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
      nodes.get(edge.to).depth = Math.max(
        nodes.get(edge.to).depth,
        nodes.get(id).depth + 1,
      );
      incoming.set(edge.to, incoming.get(edge.to) - 1);
      if (incoming.get(edge.to) === 0) ready.push(edge.to);
    }
  }
  return { nodes: [...nodes.values()], edges };
}

export function mountDiagram(container, topology, selected) {
  const graph = nodesAndEdges(topology);
  const columns = new Map();
  for (const node of graph.nodes) {
    if (!columns.has(node.depth)) {
      const column = document.createElement("div");
      column.className = "am-graph-column";
      columns.set(node.depth, column);
    }
    const element = document.createElement(
      node.kind === "stage" ? "button" : "div",
    );
    element.className = `am-graph-node ${node.kind === "stage" ? "am-stage" : "am-relay"}`;
    element.dataset.node = node.id;
    if (node.kind === "stage") {
      element.type = "button";
      element.dataset.stage = node.id;
      element.setAttribute("aria-pressed", String(node.id === selected));
    }
    const label = document.createElement("strong");
    label.textContent = node.label;
    const detail = document.createElement("small");
    detail.textContent = node.detail;
    element.append(label, detail);
    columns.get(node.depth).append(element);
  }
  [...columns.entries()]
    .sort(([left], [right]) => left - right)
    .forEach(([, column]) => container.append(column));
  const namespace = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(namespace, "svg");
  svg.classList.add("am-graph-edges");
  svg.setAttribute("aria-hidden", "true");
  container.prepend(svg);
  const draw = () => {
    svg.replaceChildren();
    const bounds = container.getBoundingClientRect();
    svg.setAttribute("viewBox", `0 0 ${bounds.width} ${bounds.height}`);
    const elements = new Map(
      [...container.querySelectorAll("[data-node]")].map((element) => [
        element.dataset.node,
        element,
      ]),
    );
    const vertical = getComputedStyle(container).flexDirection === "column";
    for (const edge of graph.edges) {
      const from = elements.get(edge.from).getBoundingClientRect();
      const to = elements.get(edge.to).getBoundingClientRect();
      const start = vertical
        ? [from.left + from.width / 2 - bounds.left, from.bottom - bounds.top]
        : [from.right - bounds.left, from.top + from.height / 2 - bounds.top];
      const end = vertical
        ? [to.left + to.width / 2 - bounds.left, to.top - bounds.top]
        : [to.left - bounds.left, to.top + to.height / 2 - bounds.top];
      const path = document.createElementNS(namespace, "path");
      const middle = vertical
        ? (start[1] + end[1]) / 2
        : (start[0] + end[0]) / 2;
      path.setAttribute(
        "d",
        vertical
          ? `M${start} C${start[0]},${middle} ${end[0]},${middle} ${end}`
          : `M${start} C${middle},${start[1]} ${middle},${end[1]} ${end}`,
      );
      svg.append(path);
      const arrow = document.createElementNS(namespace, "path");
      arrow.setAttribute(
        "d",
        vertical
          ? `M${end[0] - 3},${end[1] - 5} L${end} L${end[0] + 3},${end[1] - 5}`
          : `M${end[0] - 5},${end[1] - 3} L${end} L${end[0] - 5},${end[1] + 3}`,
      );
      svg.append(arrow);
    }
  };
  const observer = new ResizeObserver(draw);
  observer.observe(container);
  draw();
  return () => observer.disconnect();
}
