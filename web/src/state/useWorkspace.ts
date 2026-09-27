import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiClient } from "../api/client";
import type { Capture, Topology } from "../api/types";
import { errorMessage } from "../api/errors";
import { labels } from "../labels";

// API側と同じ上限で、転送前に大きなファイルを拒否する。
const MAX_CAPTURE_BYTES = 16 * 1024 * 1024;
const EMPTY_CAPTURE: Capture = {
  name: null,
  source: "capture",
  packets: [],
  truncated: false,
};

export function useWorkspace(client: ApiClient) {
  const [topology, setTopology] = useState<Topology | null>(null);
  const [capture, setCapture] = useState<Capture>(EMPTY_CAPTURE);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const uploadController = useRef<AbortController | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    uploadController.current = null;
    setBusy(false);
    Promise.all([
      client.topology(controller.signal),
      client.capture(controller.signal),
    ])
      .then(([nextTopology, nextCapture]) => {
        if (controller.signal.aborted) return;
        setTopology(nextTopology);
        setCapture(nextCapture);
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) setError(errorMessage(error));
      });
    return () => {
      controller.abort();
      uploadController.current?.abort();
    };
  }, [client]);

  const upload = useCallback(
    async (file: File, source: string) => {
      if (uploadController.current) return null;
      if (file.size > MAX_CAPTURE_BYTES) {
        setError(labels.tooLarge);
        return null;
      }
      const controller = new AbortController();
      uploadController.current = controller;
      setBusy(true);
      setError(null);
      try {
        const nextCapture = await client.upload(
          file,
          source,
          controller.signal,
        );
        if (controller.signal.aborted) return null;
        setCapture(nextCapture);
        return nextCapture;
      } catch (error) {
        if (!controller.signal.aborted) setError(errorMessage(error));
        return null;
      } finally {
        if (uploadController.current === controller) {
          uploadController.current = null;
          if (!controller.signal.aborted) setBusy(false);
        }
      }
    },
    [client],
  );
  return { topology, capture, busy, error, upload };
}
