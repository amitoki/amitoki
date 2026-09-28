import { labels } from "../labels";
import type { Capture, StatusResponse, Topology } from "./types";

// タブ内の再読み込みで認証を引き継ぎ、URLのフラグメントはすぐ取り除く。
const TOKEN_STORAGE_KEY = "amitoki-token";

export function restoreToken(): string | null {
  const incoming = location.hash.slice(1);
  if (incoming) {
    sessionStorage.setItem(TOKEN_STORAGE_KEY, incoming);
    history.replaceState(null, "", location.pathname + location.search);
  }
  return sessionStorage.getItem(TOKEN_STORAGE_KEY);
}

export function createClient(token: string | null) {
  async function request<T>(
    path: string,
    options: RequestInit = {},
  ): Promise<T> {
    if (!token) throw new Error(labels.login);
    const headers = new Headers(options.headers);
    headers.set("Authorization", `Bearer ${token}`);
    const response = await fetch(path, { ...options, headers });
    if (response.status === 401) throw new Error(labels.login);
    if (!response.ok) {
      const detail: unknown = await response.json().catch(() => null);
      const message =
        detail &&
        typeof detail === "object" &&
        "error" in detail &&
        typeof detail.error === "string"
          ? detail.error
          : labels.requestFailed;
      throw new Error(message);
    }
    return response.json() as Promise<T>;
  }
  return {
    topology: (signal: AbortSignal) =>
      request<Topology>("/api/topology", { signal }),
    capture: (signal: AbortSignal) =>
      request<Capture>("/api/capture", { signal }),
    status: (signal: AbortSignal) =>
      request<StatusResponse>("/api/status", { signal }),
    upload: (file: File, source: string, signal: AbortSignal) =>
      request<Capture>(
        `/api/capture?${new URLSearchParams({ name: file.name, source })}`,
        {
          method: "POST",
          headers: { "Content-Type": "application/octet-stream" },
          body: file,
          signal,
        },
      ),
  };
}
export type ApiClient = ReturnType<typeof createClient>;
