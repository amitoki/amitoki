import type { Topology } from "../api/types";
import { labels } from "../labels";
import { Heading } from "../components/Heading";
import { PipelineDiagram } from "./PipelineDiagram";
interface Props {
  topology: Topology;
  active: boolean;
  running: boolean;
  stage: string | null;
  onActive: (active: boolean) => void;
  onStage: (stage: string) => void;
  onPackets: () => void;
}
export function PipelineView({
  topology,
  active,
  running,
  stage,
  onActive,
  onStage,
  onPackets,
}: Props) {
  const selected =
    topology.stages.find((entry) => entry.id === stage) ?? topology.stages[0];
  return (
    <>
      <Heading title={topology.channel}>
        <label className="am-source">
          <select
            id="topology-source"
            aria-label={labels.pipeline}
            value={active ? "active" : "replay"}
            onChange={(event) => onActive(event.target.value === "active")}
          >
            <option value="replay">{labels.replay}</option>
            <option value="active" disabled={!running}>
              {labels.active}
            </option>
          </select>
        </label>
      </Heading>
      <div className="am-pipeline-layout grid grid-cols-[minmax(0,1fr)_218px] rounded-lg border border-line bg-panel max-wide:grid-cols-1">
        <section className="am-canvas min-w-0 rounded-l-lg px-3.5 py-5 bg-[radial-gradient(var(--color-line)_0.65px,transparent_0.65px)] bg-size-[16px_16px] max-compact:p-3.5">
          <PipelineDiagram
            topology={topology}
            selected={selected?.id}
            onSelect={onStage}
          />
        </section>
        <aside className="am-inspector min-w-0 border-l border-line p-[17px] max-wide:grid max-wide:grid-cols-2 max-wide:gap-x-[22px] max-wide:border-l-0 max-wide:border-t max-compact:block [&_h3]:mt-[7px] [&_h3]:mb-[3px] [&_.am-button]:mt-5 [&_.am-button]:w-full [&_.am-button]:text-[12px] max-wide:[&_.am-button]:mt-2.5">
          {selected ? (
            <>
              <div className="am-caption">Stage</div>
              <h3>{selected.id}</h3>
              <div className="am-kv flex justify-between gap-2.5 border-b border-line py-1.5 text-[12px] [&_span:last-child]:text-right [&_span:last-child]:font-mono [&_span:last-child]:wrap-anywhere">
                <span>{labels.plugin}</span>
                <span>{selected.plugin}</span>
              </div>
              <div className="am-kv flex justify-between gap-2.5 border-b border-line py-1.5 text-[12px] [&_span:last-child]:text-right [&_span:last-child]:font-mono [&_span:last-child]:wrap-anywhere">
                <span>{labels.policy}</span>
                <span>{selected.on_error}</span>
              </div>
              <button
                className="am-button"
                type="button"
                data-view="packets"
                onClick={onPackets}
              >
                {labels.packets}
              </button>
            </>
          ) : (
            <span>—</span>
          )}
        </aside>
      </div>
      <section className="am-routes mt-5 [&_h3]:mb-2.5 [&_.am-arrow]:w-[35px]">
        <h3>{labels.routes}</h3>
        <div className="am-frame">
          <table className="am-table">
            <tbody>
              {topology.routes.map((route) => (
                <tr key={route.from}>
                  <td>{route.from}</td>
                  <td className="am-arrow text-center text-muted">→</td>
                  <td>
                    {route.to.length ? route.to.join(" / ") : labels.dropped}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </>
  );
}
