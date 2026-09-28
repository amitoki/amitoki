import type { Status, Topology } from "../api/types";
import type { StatusEvent } from "../state/statusEvents";
import { labels } from "../labels";
import { Heading } from "../components/Heading";
interface Props {
  topology: Topology;
  status: Status | null;
  events: StatusEvent[];
}
const counters = [
  "captured",
  "published",
  "injected",
  "filtered",
  "rejected",
  "retries",
] as const;
const counterLabels = {
  captured: labels.captureCount,
  published: labels.published,
  injected: labels.injected,
  filtered: labels.filtered,
  rejected: labels.rejected,
  retries: labels.retry,
};
export function OperationsView({ topology, status, events }: Props) {
  return (
    <>
      <Heading title={topology.node_id}>
        {status?.generation != null && (
          <span className="am-pill rounded bg-subtle px-[7px] py-[3px] text-[11px] whitespace-nowrap">
            {labels.generation} {status.generation}
          </span>
        )}
      </Heading>
      {!status ? (
        <p className="am-empty py-[30px] text-muted">{labels.unavailable}</p>
      ) : (
        <div className="am-frame">
          <table className="am-table">
            <tbody>
              {counters.map((key) => (
                <tr key={key}>
                  <th>{counterLabels[key]}</th>
                  <td className="am-number tabular-nums">
                    {status[key].toLocaleString("ja-JP")}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <div className="am-route-list mt-3.5 grid gap-2.5">
        {topology.relays.map((relay) => {
          const metrics = status?.relays.find((entry) => entry.id === relay.id);
          const state = metrics
            ? metrics.failures
              ? labels.impaired
              : labels.running
            : labels.unknown;
          return (
            <div
              className="am-route-row grid grid-cols-[1fr_auto] gap-[15px] rounded-lg border border-line bg-panel p-4 max-compact:grid-cols-1 max-compact:p-3 [&_h3]:flex [&_h3]:items-center [&_h3]:gap-[9px] [&_p]:mt-[5px] [&_p]:text-[12px] [&_p]:text-muted"
              key={relay.id}
            >
              <div>
                <h3>
                  {relay.id}{" "}
                  <span
                    className={`am-status text-[11px] text-good [&.am-error]:text-bad ${metrics?.failures ? "am-error" : ""}`}
                  >
                    {state}
                  </span>
                </h3>
                <p>{relay.plugin}</p>
              </div>
              <div className="am-route-numbers text-right font-mono text-[12px]/[1.8] whitespace-normal max-compact:text-left">
                {metrics ? (
                  <>
                    {labels.queued} {metrics.queued.toLocaleString("ja-JP")} /{" "}
                    {metrics.capacity.toLocaleString("ja-JP")}
                    <br />
                    {labels.published}{" "}
                    {metrics.published.toLocaleString("ja-JP")} ·{" "}
                    {labels.dropped} {metrics.dropped.toLocaleString("ja-JP")} ·{" "}
                    {labels.failures} {metrics.failures.toLocaleString("ja-JP")}
                  </>
                ) : (
                  "—"
                )}
              </div>
            </div>
          );
        })}
      </div>
      <section className="am-events mt-[22px]">
        <h3>{labels.events}</h3>
        {events.length ? (
          events.map((event) => (
            <div
              className="am-event grid grid-cols-[66px_1fr] gap-3 border-b border-line py-2.5 text-[12px] [&_time]:font-mono [&_time]:text-muted"
              key={event.id}
            >
              <time>{event.time}</time>
              <span>{event.message}</span>
            </div>
          ))
        ) : (
          <p className="am-caption">{labels.noEvents}</p>
        )}
      </section>
    </>
  );
}
