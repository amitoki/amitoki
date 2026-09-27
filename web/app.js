import { labels as text } from "./labels.js";
import { packetsView, pipelineView, operationsView } from "./views.js";
import { mountDiagram } from "./diagram.js";

const root = document.getElementById("amitoki-design");
const content = document.getElementById("am-view");
const message = document.getElementById("message");
// 更新のたびに表示を飛ばさず、利用者の選択はタブ内で維持する。
const STATUS_INTERVAL_MS = 2000;
const MAX_EVENTS = 50;
const MAX_CAPTURE_BYTES = 16 * 1024 * 1024;
const state = {
  view: "packets",
  topology: null,
  capture: { name: null, packets: [] },
  status: null,
  source: "capture",
  packet: null,
  step: 0,
  stage: null,
  page: 0,
  pageSize: 25,
  busy: false,
  liveTopology: false,
  events: [],
};
const incomingToken = location.hash.slice(1);
if (incomingToken) {
  sessionStorage.setItem("amitoki-token", incomingToken);
  history.replaceState(null, "", location.pathname);
}
const token = sessionStorage.getItem("amitoki-token");
let disposeDiagram = () => {};

function showError(error) {
  message.textContent = error?.message ?? "";
  message.hidden = !error;
}

async function request(path, options = {}) {
  const response = await fetch(path, {
    ...options,
    headers: { ...options.headers, Authorization: `Bearer ${token}` },
  });
  if (response.status === 401) throw new Error(text.login);
  if (!response.ok) {
    const detail = await response.json().catch(() => null);
    throw new Error(detail?.error ?? text.requestFailed);
  }
  return response.json();
}

function render() {
  if (!state.topology) return;
  disposeDiagram();
  root
    .querySelectorAll(".am-nav [data-view]")
    .forEach((button) =>
      button.setAttribute(
        "aria-pressed",
        String(button.dataset.view === state.view),
      ),
    );
  const focused = document.activeElement;
  const focusAttribute = [
    "data-view",
    "data-packet",
    "data-step",
    "data-stage",
    "data-page",
  ].find((attribute) => focused?.hasAttribute(attribute));
  const focusValue = focusAttribute
    ? focused.getAttribute(focusAttribute)
    : null;
  content.innerHTML = {
    packets: packetsView,
    pipeline: pipelineView,
    operations: operationsView,
  }[state.view](state);
  if (state.view === "pipeline") {
    const topology =
      state.liveTopology && state.status?.running
        ? state.status.status.topology
        : state.topology;
    const selected =
      topology.stages.find((stage) => stage.id === state.stage) ??
      topology.stages[0];
    disposeDiagram = mountDiagram(
      document.getElementById("pipeline-diagram"),
      topology,
      selected?.id,
    );
  }
  if (focusAttribute) {
    const replacement = [...root.querySelectorAll(`[${focusAttribute}]`)].find(
      (element) => element.getAttribute(focusAttribute) === focusValue,
    );
    replacement?.focus({ preventScroll: true });
  }
}

function addEvent(message) {
  state.events.unshift({
    time: new Date().toLocaleTimeString("ja-JP"),
    message,
  });
  state.events.length = Math.min(state.events.length, MAX_EVENTS);
}

async function refreshStatus() {
  try {
    const next = await request("/api/status");
    const topologyChanged =
      next.running !== state.status?.running ||
      next.status?.generation !== state.status?.status?.generation;
    if (next.running && !state.status?.running) addEvent(text.connected);
    else if (!next.running && state.status?.running)
      addEvent(text.disconnected);
    if (next.running && state.status?.running) {
      if (next.status.generation !== state.status.status.generation)
        addEvent(`${text.reload}: ${next.status.generation}`);
      for (const relay of next.status.relays) {
        const previous = state.status.status.relays.find(
          (entry) => entry.id === relay.id,
        );
        if (previous && relay.dropped > previous.dropped)
          addEvent(
            `${relay.id}: ${text.dropped} +${relay.dropped - previous.dropped}`,
          );
        if (previous && relay.failures > previous.failures)
          addEvent(
            `${relay.id}: ${text.failures} +${relay.failures - previous.failures}`,
          );
      }
    }
    state.status = next;
    if (!next.running) state.liveTopology = false;
    document.getElementById("connection-state").textContent = next.running
      ? text.running
      : text.unknown;
    document
      .getElementById("connection-dot")
      .classList.toggle("am-offline", !next.running);
    if (
      state.view === "operations" ||
      (state.view === "pipeline" && topologyChanged)
    )
      render();
  } catch (error) {
    state.status = null;
    state.liveTopology = false;
    document.getElementById("connection-state").textContent = text.unknown;
    document.getElementById("connection-dot").classList.add("am-offline");
    showError(error);
    if (state.view === "operations" || state.view === "pipeline") render();
  } finally {
    window.setTimeout(refreshStatus, STATUS_INTERVAL_MS);
  }
}

root.addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button) return;
  if (button.dataset.view) {
    if (state.view === "pipeline" && button.dataset.view === "packets") {
      const packet = state.capture.packets.find(
        (packet) => packet.packet === state.packet,
      );
      state.step = Math.max(
        0,
        packet?.steps.findIndex((step) => step.block === state.stage) ?? 0,
      );
    }
    state.view = button.dataset.view;
  } else if (button.dataset.packet) {
    state.packet = Number(button.dataset.packet);
    state.step = 0;
  } else if (button.dataset.step !== undefined)
    state.step = Number(button.dataset.step);
  else if (button.dataset.stage) state.stage = button.dataset.stage;
  else if (button.dataset.page) state.page += Number(button.dataset.page);
  else return;
  render();
});

root.addEventListener("change", async (event) => {
  if (event.target.id === "source") {
    state.source = event.target.value;
    return;
  }
  if (event.target.id === "topology-source") {
    state.liveTopology = event.target.value === "active";
    render();
    return;
  }
  if (event.target.id !== "pcap" || !event.target.files.length) return;
  const file = event.target.files[0];
  if (file.size > MAX_CAPTURE_BYTES) {
    showError(new Error(text.tooLarge));
    return;
  }
  state.busy = true;
  showError(null);
  render();
  try {
    state.capture = await request(
      `/api/capture?${new URLSearchParams({ name: file.name, source: state.source })}`,
      {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: file,
      },
    );
    state.packet = state.capture.packets[0]?.packet ?? null;
    state.step = 0;
    state.page = 0;
  } catch (error) {
    showError(error);
  } finally {
    state.busy = false;
    render();
  }
});

document.querySelectorAll("[data-label]").forEach((element) => {
  element.textContent = text[element.dataset.label];
});
try {
  if (!token) throw new Error(text.login);
  [state.topology, state.capture] = await Promise.all([
    request("/api/topology"),
    request("/api/capture"),
  ]);
  state.packet = state.capture.packets[0]?.packet ?? null;
  document.getElementById("node-name").textContent = state.topology.node_id;
  document.getElementById("channel-name").textContent = state.topology.channel;
  document.getElementById("pipeline-count").textContent =
    `${state.topology.stages.length} Stage / ${state.topology.relays.length} Relay`;
  render();
  refreshStatus();
} catch (error) {
  showError(error);
}
