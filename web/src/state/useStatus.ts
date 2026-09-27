import { useEffect, useRef, useState } from "react";
import type { ApiClient } from "../api/client";
import type { StatusResponse } from "../api/types";
import { errorMessage } from "../api/errors";
import { statusEvents, type StatusEvent } from "./statusEvents";

// 応答完了から次の更新まで待ち、遅いAPI要求を重ねない。
const STATUS_INTERVAL_MS = 2000;
// ブラウザを開いた後の観測履歴だけを有界で保持する。
const MAX_EVENTS = 50;

interface Observation {
  response: StatusResponse | null;
  events: StatusEvent[];
  error: string | null;
}
export function useStatus(client: ApiClient, enabled: boolean) {
  const eventSequence = useRef(0);
  const [observation, setObservation] = useState<Observation>({
    response: null,
    events: [],
    error: null,
  });
  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let previous: StatusResponse | null = null;
    async function refresh() {
      try {
        const next = await client.status(controller.signal);
        if (controller.signal.aborted) return;
        const time = new Date().toLocaleTimeString("ja-JP");
        const events = statusEvents(previous, next).map((message) => ({
          id: eventSequence.current++,
          time,
          message,
        }));
        // 世代が同じなら構成図のレイアウトも維持する。
        if (
          next.running &&
          previous?.running &&
          next.status.generation === previous.status.generation
        ) {
          next.status.topology = previous.status.topology;
        }
        previous = next;
        setObservation((current) => ({
          response: next,
          error: null,
          events: [...events.reverse(), ...current.events].slice(0, MAX_EVENTS),
        }));
      } catch (error) {
        if (controller.signal.aborted) return;
        previous = null;
        setObservation((current) => ({
          ...current,
          response: null,
          error: errorMessage(error),
        }));
      } finally {
        if (!controller.signal.aborted)
          timer = setTimeout(refresh, STATUS_INTERVAL_MS);
      }
    }
    void refresh();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [client, enabled]);
  return observation;
}
