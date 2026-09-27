import type { PacketSnapshot } from "../api/types";
import { labels } from "../labels";
import { snapshotFields } from "./packetFields";
interface Props {
  label: string;
  snapshot?: PacketSnapshot;
  other?: PacketSnapshot;
}
export function SnapshotPanel({ label, snapshot, other }: Props) {
  if (!snapshot)
    return (
      <div className="am-diff-panel">
        <span className="am-caption">{label}</span>
        <p>{labels.noOutput}</p>
      </div>
    );
  const reference = snapshotFields(other);
  const current = snapshotFields(snapshot);
  const fields = [
    ...new Set([...Object.keys(current), ...Object.keys(reference)]),
  ].sort();
  return (
    <div className="am-diff-panel">
      <span className="am-caption">{label}</span>
      <div className="am-fields mt-3 font-mono text-[12px]/[1.8]">
        {fields.map((key) => {
          const value = current[key];
          const rendered =
            value === undefined
              ? "—"
              : typeof value === "object"
                ? JSON.stringify(value)
                : String(value);
          const changed =
            other && JSON.stringify(reference[key]) !== JSON.stringify(value);
          return (
            <div
              className="am-field grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] gap-3 [&_span]:wrap-anywhere"
              key={key}
            >
              <span>{key.replace(/^annotations\./, "")}</span>
              <span>{changed ? <mark>{rendered}</mark> : rendered}</span>
            </div>
          );
        })}
      </div>
      <details className="am-bytes mt-3.5 [&_summary]:cursor-pointer [&_summary]:text-[11px] [&_summary]:text-muted">
        <summary>
          {labels.hex}
          {snapshot.truncated ? ` · ${labels.preview}` : ""}
        </summary>
        <pre className="am-code mt-3 font-mono text-[12px]/[1.8] whitespace-pre-wrap wrap-anywhere">
          {snapshot.hex}
        </pre>
      </details>
    </div>
  );
}
