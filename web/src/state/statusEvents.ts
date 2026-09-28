import type { StatusResponse } from "../api/types";
import { labels } from "../labels";

export interface StatusEvent {
  id: number;
  time: string;
  message: string;
}

export function statusEvents(
  previous: StatusResponse | null,
  next: StatusResponse,
): string[] {
  if (next.running && !previous?.running) return [labels.connected];
  if (!next.running && previous?.running) return [labels.disconnected];
  if (!next.running || !previous?.running) return [];
  const events: string[] = [];
  if (next.status.generation !== previous.status.generation)
    events.push(`${labels.reload}: ${next.status.generation}`);
  for (const relay of next.status.relays) {
    const before = previous.status.relays.find(
      (entry) => entry.id === relay.id,
    );
    if (!before) continue;
    if (relay.dropped > before.dropped)
      events.push(
        `${relay.id}: ${labels.dropped} +${relay.dropped - before.dropped}`,
      );
    if (relay.failures > before.failures)
      events.push(
        `${relay.id}: ${labels.failures} +${relay.failures - before.failures}`,
      );
  }
  return events;
}
