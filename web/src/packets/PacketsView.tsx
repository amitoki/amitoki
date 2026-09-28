import { Fragment } from "react";
import type { Capture, Topology } from "../api/types";
import { labels } from "../labels";
import { Heading } from "../components/Heading";
import { outcome, protocol } from "./packetFields";
import { SnapshotPanel } from "./SnapshotPanel";

// 1,000件のキャプチャを一度にDOMへ展開しない。
const PAGE_SIZE = 25;
export interface PacketSelection {
  packet: number | null;
  step: number;
  page: number;
}
interface Props {
  capture: Capture;
  topology: Topology;
  busy: boolean;
  source: string;
  selection: PacketSelection;
  onSelection: (selection: PacketSelection) => void;
  onSource: (source: string) => void;
  onUpload: (file: File) => void;
}
export function PacketsView({
  capture,
  topology,
  busy,
  source,
  selection,
  onSelection,
  onSource,
  onUpload,
}: Props) {
  const sources = [
    "capture",
    ...topology.relays.map((relay) => `${relay.id}.received`),
  ];
  const start = selection.page * PAGE_SIZE;
  const packet =
    capture.packets.find((packet) => packet.packet === selection.packet) ??
    capture.packets[0];
  const step = packet?.steps[selection.step];
  const input = step?.input ?? packet?.input;
  const output = step
    ? step.output
    : packet && !packet.rejection && !packet.error && packet.terminals.length
      ? packet.input
      : undefined;
  const failure = packet?.rejection ?? step?.error ?? packet?.error;
  return (
    <>
      <Heading title={capture.name ?? labels.capture}>
        <div className="am-toolbar flex flex-wrap items-center gap-2 max-compact:w-full max-compact:justify-between">
          <label className="am-source">
            {labels.source}
            <select
              id="source"
              value={source}
              onChange={(event) => onSource(event.target.value)}
            >
              {sources.map((source) => (
                <option key={source}>{source}</option>
              ))}
            </select>
          </label>
          <label
            className={`am-button am-upload relative cursor-pointer focus-within:outline-2 focus-within:outline-offset-3 focus-within:outline-accent [&_input]:absolute [&_input]:inset-0 [&_input]:size-full [&_input]:cursor-pointer [&_input]:opacity-0 pointer-coarse:min-h-11 pointer-coarse:text-[16px] ${busy ? "am-disabled cursor-default opacity-50" : ""}`}
          >
            {busy ? labels.analyzing : labels.open}
            <input
              id="pcap"
              type="file"
              accept=".pcap,application/vnd.tcpdump.pcap"
              disabled={busy}
              onChange={(event) => {
                const file = event.target.files?.[0];
                // 同じファイルを直して再選択した場合もchangeを発火させる。
                event.target.value = "";
                if (file) onUpload(file);
              }}
            />
          </label>
        </div>
      </Heading>
      {!packet ? (
        <p className="am-empty py-[30px] text-muted">{labels.empty}</p>
      ) : (
        <>
          <div className="am-frame">
            <table className="am-table">
              <thead>
                <tr>
                  <th>{labels.packet}</th>
                  <th className="am-optional max-compact:hidden">
                    {labels.time} · UTC
                  </th>
                  <th>{labels.type}</th>
                  <th>{labels.length}</th>
                  <th>{labels.outcome}</th>
                </tr>
              </thead>
              <tbody>
                {capture.packets
                  .slice(start, start + PAGE_SIZE)
                  .map((entry) => (
                    <tr
                      key={entry.packet}
                      className={
                        entry.packet === packet.packet ? "am-selected" : ""
                      }
                    >
                      <td>
                        <button
                          type="button"
                          data-packet={entry.packet}
                          aria-pressed={entry.packet === packet.packet}
                          onClick={() =>
                            onSelection({
                              ...selection,
                              packet: entry.packet,
                              step: 0,
                            })
                          }
                        >
                          #{entry.packet}
                        </button>
                      </td>
                      <td className="am-optional max-compact:hidden am-number tabular-nums">
                        {new Date(entry.timestamp_ns / 1e6)
                          .toISOString()
                          .slice(11, 23)}
                      </td>
                      <td>{protocol(entry)}</td>
                      <td className="am-number tabular-nums">
                        {entry.length.toLocaleString("ja-JP")} B
                      </td>
                      <td>{outcome(entry)}</td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
          <div className="am-pagination mt-3 flex items-center justify-between gap-2 text-[11px] text-muted [&>div]:flex [&>div]:gap-1.5">
            <span>
              {start + 1}–{Math.min(start + PAGE_SIZE, capture.packets.length)}{" "}
              / {capture.packets.length.toLocaleString("ja-JP")}
              {capture.truncated ? ` · ${labels.limit}` : ""}
            </span>
            <div>
              <button
                className="am-button"
                type="button"
                data-page="-1"
                disabled={selection.page === 0}
                onClick={() =>
                  onSelection({ ...selection, page: selection.page - 1 })
                }
              >
                {labels.previous}
              </button>
              <button
                className="am-button"
                type="button"
                data-page="1"
                disabled={start + PAGE_SIZE >= capture.packets.length}
                onClick={() =>
                  onSelection({ ...selection, page: selection.page + 1 })
                }
              >
                {labels.next}
              </button>
            </div>
          </div>
          <div className="am-record-summary mt-5 mb-3 flex flex-wrap items-center gap-3 [&_h3]:text-[14px]">
            <h3>#{packet.packet}</h3>
            <span className="am-caption">{capture.source}</span>
          </div>
          <div className="am-trace flex flex-wrap items-center gap-1.5 py-3">
            {packet.steps.map((step, index) => (
              <Fragment key={index}>
                {index > 0 && (
                  <span
                    className="am-arrow text-center text-muted"
                    aria-hidden="true"
                  >
                    →
                  </span>
                )}
                <button
                  type="button"
                  className="rounded-[5px] border border-line bg-panel px-2 py-[5px] text-[11px] aria-pressed:border-accent aria-pressed:bg-accent-soft aria-pressed:text-accent"
                  data-step={index}
                  aria-pressed={selection.step === index}
                  onClick={() => onSelection({ ...selection, step: index })}
                >
                  {step.block}
                </button>
              </Fragment>
            ))}
          </div>
          <section className="am-frame">
            <div className="am-section-head flex flex-wrap items-center justify-between gap-2.5 px-3.5 py-3">
              <h3>{step?.block ?? labels.required}</h3>
              <span className="am-caption">
                {step ? `${step.elapsed_us.toLocaleString("ja-JP")} μs` : ""}
              </span>
            </div>
            <div className="am-diff grid grid-cols-2 border-t border-line max-compact:grid-cols-1">
              <SnapshotPanel
                label={labels.input}
                snapshot={input}
                other={output}
              />
              <SnapshotPanel
                label={labels.output}
                snapshot={output}
                other={input}
              />
            </div>
          </section>
          <div
            className={`am-annotation mt-3.5 rounded-[5px] bg-subtle px-3 py-2.5 text-[12px] wrap-anywhere [&.am-error]:text-bad ${failure ? "am-error" : ""}`}
          >
            {failure ??
              (packet.terminals.length
                ? `${labels.outputTo}: ${packet.terminals.join(" / ")}`
                : labels.noTerminal)}
          </div>
        </>
      )}
    </>
  );
}
