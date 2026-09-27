import { labels as text } from "./labels.js";

// 設定名・PCAP名・プラグイン出力をHTMLとして解釈しない。
export function escape(value) {
  return String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ],
  );
}
const count = (value) => Number(value ?? 0).toLocaleString("ja-JP");
const heading = (title, action = "") =>
  `<div class="am-heading"><h2>${escape(title)}</h2>${action}</div>`;
const button = (label, attributes = "") =>
  `<button class="am-button" type="button" ${attributes}>${escape(label)}</button>`;

function outcome(packet) {
  if (packet.error || packet.steps.some((step) => step.error))
    return text.failed;
  if (packet.rejection) return text.rejected;
  if (!packet.terminals.length) return text.dropped;
  return packet.steps.some((step) => step.rewrite)
    ? text.modified
    : text.passed;
}

function protocol(packet) {
  const fields = packet.input?.fields ?? {};
  return (
    { 6: "TCP", 17: "UDP", 1: "ICMP", 58: "ICMPv6" }[fields.protocol] ??
    fields.ether_type ??
    "—"
  );
}

function fields(snapshot) {
  if (!snapshot) return {};
  const values = { length: snapshot.length, ...snapshot.fields };
  // 深い任意JSONは末端のJSON表記に留め、通常のパケット定義はフィールド単位で比較する。
  const MAX_FIELD_DEPTH = 16;
  function appendFields(value, path, depth) {
    if (
      value &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      Object.keys(value).length &&
      depth < MAX_FIELD_DEPTH
    ) {
      for (const [name, child] of Object.entries(value))
        appendFields(child, `${path}.${name}`, depth + 1);
    } else values[path] = value;
  }
  for (const [name, value] of Object.entries(snapshot.annotations ?? {}))
    appendFields(value, `annotations.${name}`, 0);
  return Object.fromEntries(
    Object.entries(values).filter(([, value]) => value !== null),
  );
}

function snapshotPanel(label, snapshot, other) {
  if (!snapshot)
    return `<div class="am-diff-panel"><span class="am-caption">${label}</span><p>${text.noOutput}</p></div>`;
  const reference = fields(other);
  const current = fields(snapshot);
  const rows = [
    ...new Set([...Object.keys(current), ...Object.keys(reference)]),
  ]
    .sort()
    .map((key) => {
      const value = current[key];
      const rendered =
        value === undefined
          ? "—"
          : typeof value === "object"
            ? JSON.stringify(value)
            : String(value);
      const changed =
        other && JSON.stringify(reference[key]) !== JSON.stringify(value);
      return `<div class="am-field"><span>${escape(key.replace(/^annotations\./, ""))}</span><span>${changed ? `<mark>${escape(rendered)}</mark>` : escape(rendered)}</span></div>`;
    })
    .join("");
  return `<div class="am-diff-panel"><span class="am-caption">${label}</span><div class="am-fields">${rows}</div><details class="am-bytes"><summary>Hex${snapshot.truncated ? ` · ${text.preview}` : ""}</summary><pre class="am-code">${escape(snapshot.hex)}</pre></details></div>`;
}

export function packetsView(state) {
  const capture = state.capture;
  const sources = [
    "capture",
    ...state.topology.relays.map((relay) => `${relay.id}.received`),
  ];
  const toolbar = `<div class="am-toolbar"><label class="am-source">${text.source}<select id="source">${sources.map((source) => `<option ${state.source === source ? "selected" : ""}>${escape(source)}</option>`).join("")}</select></label><label class="am-button am-upload ${state.busy ? "am-disabled" : ""}">${state.busy ? text.analyzing : text.open}<input id="pcap" type="file" accept=".pcap,application/vnd.tcpdump.pcap" ${state.busy ? "disabled" : ""}></label></div>`;
  let html = heading(capture.name ?? text.capture, toolbar);
  if (!capture.packets.length)
    return html + `<p class="am-empty">${text.empty}</p>`;
  const start = state.page * state.pageSize;
  const rows = capture.packets
    .slice(start, start + state.pageSize)
    .map(
      (packet) =>
        `<tr class="${packet.packet === state.packet ? "am-selected" : ""}"><td><button type="button" data-packet="${packet.packet}" aria-pressed="${packet.packet === state.packet}">#${packet.packet}</button></td><td class="am-optional am-number">${escape(new Date(packet.timestamp_ns / 1e6).toISOString().slice(11, 23))}</td><td>${escape(protocol(packet))}</td><td class="am-number">${count(packet.length)} B</td><td>${outcome(packet)}</td></tr>`,
    )
    .join("");
  html += `<div class="am-frame"><table class="am-table"><thead><tr><th>${text.packet}</th><th class="am-optional">${text.time} · UTC</th><th>${text.type}</th><th>${text.length}</th><th>${text.outcome}</th></tr></thead><tbody>${rows}</tbody></table></div>`;
  html += `<div class="am-pagination"><span>${start + 1}–${Math.min(start + state.pageSize, capture.packets.length)} / ${count(capture.packets.length)}${capture.truncated ? ` · ${text.limit}` : ""}</span><div>${button(text.previous, `data-page="-1" ${state.page === 0 ? "disabled" : ""}`)}${button(text.next, `data-page="1" ${start + state.pageSize >= capture.packets.length ? "disabled" : ""}`)}</div></div>`;
  const packet = capture.packets.find(
    (packet) => packet.packet === state.packet,
  );
  if (!packet) return html;
  html += `<div class="am-record-summary"><h3>#${packet.packet}</h3><span class="am-caption">${escape(capture.source)}</span></div>`;
  html += `<div class="am-trace">${packet.steps.map((step, index) => `<button type="button" data-step="${index}" aria-pressed="${state.step === index}">${escape(step.block)}</button>`).join('<span class="am-arrow" aria-hidden="true">→</span>')}</div>`;
  const step = packet.steps[state.step];
  const output = step
    ? step.output
    : !packet.rejection && !packet.error && packet.terminals.length
      ? packet.input
      : null;
  html += `<section class="am-frame"><div class="am-section-head"><h3>${escape(step?.block ?? text.required)}</h3><span class="am-caption">${step ? `${count(step.elapsed_us)} μs` : ""}</span></div><div class="am-diff">${snapshotPanel(text.input, step?.input ?? packet.input, output)}${snapshotPanel(text.output, output, step?.input ?? packet.input)}</div></section>`;
  const failure = packet.rejection ?? step?.error ?? packet.error;
  html += `<div class="am-annotation ${failure ? "am-error" : ""}">${escape(failure ?? (packet.terminals.length ? `${text.outputTo}: ${packet.terminals.join(" / ")}` : text.noTerminal))}</div>`;
  return html;
}

export function pipelineView(state) {
  const topology =
    state.liveTopology && state.status?.running
      ? state.status.status.topology
      : state.topology;
  const selected =
    topology.stages.find((stage) => stage.id === state.stage) ??
    topology.stages[0];
  const selector = `<label class="am-source"><select id="topology-source"><option value="replay" ${state.liveTopology ? "" : "selected"}>${text.replay}</option><option value="active" ${state.liveTopology ? "selected" : ""} ${state.status?.running ? "" : "disabled"}>${text.active}</option></select></label>`;
  // ポート名は図と併せて、元の設定どおりに一覧化する。
  const routes = topology.routes
    .map(
      (route) =>
        `<tr><td>${escape(route.from)}</td><td class="am-arrow">→</td><td>${route.to.length ? route.to.map(escape).join(" / ") : text.dropped}</td></tr>`,
    )
    .join("");
  return (
    heading(topology.channel, selector) +
    `<div class="am-pipeline-layout"><section class="am-canvas"><div id="pipeline-diagram" class="am-graph" aria-label="パイプライン"></div></section><aside class="am-inspector">${selected ? `<div class="am-caption">Stage</div><h3>${escape(selected.id)}</h3><div class="am-kv"><span>${text.plugin}</span><span>${escape(selected.plugin)}</span></div><div class="am-kv"><span>${text.policy}</span><span>${escape(selected.on_error)}</span></div>${button(text.packets, 'data-view="packets"')}` : "<span>—</span>"}</aside></div><section class="am-routes"><h3>${text.routes}</h3><div class="am-frame"><table class="am-table"><tbody>${routes}</tbody></table></div></section>`
  );
}

export function operationsView(state) {
  const status = state.status?.running ? state.status.status : null;
  const topology = status?.topology ?? state.topology;
  let html = heading(
    topology.node_id,
    status?.generation
      ? `<span class="am-pill">${text.generation} ${status.generation}</span>`
      : "",
  );
  if (!status) html += `<p class="am-empty">${text.unavailable}</p>`;
  if (status)
    html += `<div class="am-frame"><table class="am-table"><tbody>${[
      ["captured", text.captureCount],
      ["published", text.published],
      ["injected", text.injected],
      ["filtered", text.filtered],
      ["rejected", text.rejected],
      ["retries", text.retry],
    ]
      .map(
        ([key, label]) =>
          `<tr><th>${label}</th><td class="am-number">${count(status[key])}</td></tr>`,
      )
      .join("")}</tbody></table></div>`;
  html += `<div class="am-route-list">${topology.relays
    .map((relay) => {
      const metrics = status?.relays.find((entry) => entry.id === relay.id);
      const label = metrics
        ? metrics.failures
          ? text.impaired
          : text.running
        : text.unknown;
      return `<div class="am-route-row"><div><h3>${escape(relay.id)} <span class="am-status ${metrics?.failures ? "am-error" : ""}">${label}</span></h3><p>${escape(relay.plugin)}</p></div><div class="am-route-numbers">${metrics ? `${text.queued} ${count(metrics.queued)} / ${count(metrics.capacity)}<br>${text.published} ${count(metrics.published)} · ${text.dropped} ${count(metrics.dropped)} · ${text.failures} ${count(metrics.failures)}` : "—"}</div></div>`;
    })
    .join("")}</div>`;
  html += `<section class="am-events"><h3>${text.events}</h3>${state.events.length ? state.events.map((event) => `<div class="am-event"><time>${escape(event.time)}</time><span>${escape(event.message)}</span></div>`).join("") : `<p class="am-caption">${text.noEvents}</p>`}</section>`;
  return html;
}
