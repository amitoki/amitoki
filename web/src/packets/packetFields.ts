import type { JsonValue, PacketReport, PacketSnapshot } from "../api/types";
import { labels } from "../labels";

export function outcome(packet: PacketReport): string {
  if (packet.error || packet.steps.some((step) => step.error))
    return labels.failed;
  if (packet.rejection) return labels.rejected;
  if (!packet.terminals.length) return labels.dropped;
  return packet.steps.some((step) => step.rewrite)
    ? labels.modified
    : labels.passed;
}
export function protocol(packet: PacketReport): string {
  const fields = packet.input?.fields ?? {};
  const protocols: Record<string, string> = {
    6: "TCP",
    17: "UDP",
    1: "ICMP",
    58: "ICMPv6",
  };
  return protocols[String(fields.protocol)] ?? String(fields.ether_type ?? "—");
}
export function snapshotFields(
  snapshot?: PacketSnapshot,
): Record<string, JsonValue> {
  if (!snapshot) return {};
  const values: Record<string, JsonValue> = {
    length: snapshot.length,
    ...snapshot.fields,
  };
  // 任意JSONの深すぎる階層は末端のJSON表記に留める。
  const MAX_FIELD_DEPTH = 16;
  function appendFields(value: JsonValue, path: string, depth: number) {
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
  const annotations = snapshot.annotations;
  if (
    annotations &&
    typeof annotations === "object" &&
    !Array.isArray(annotations)
  ) {
    for (const [name, value] of Object.entries(annotations))
      appendFields(value, `annotations.${name}`, 0);
  } else if (annotations !== null) appendFields(annotations, "annotations", 0);
  return Object.fromEntries(
    Object.entries(values).filter(([, value]) => value !== null),
  );
}
