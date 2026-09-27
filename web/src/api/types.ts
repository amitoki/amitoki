export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export interface Stage {
  id: string;
  plugin: string;
  on_error: "stop" | "drop_branch";
}
export interface Relay {
  id: string;
  plugin: string;
}
export interface Route {
  from: string;
  to: string[];
}
export interface Topology {
  node_id: string;
  channel: string;
  interface: string;
  stages: Stage[];
  relays: Relay[];
  routes: Route[];
}
export interface PacketSnapshot {
  length: number;
  fields: Record<string, JsonValue>;
  annotations: JsonValue;
  hex: string;
  truncated: boolean;
}
export interface BlockStep {
  block: string;
  input?: PacketSnapshot;
  output?: PacketSnapshot;
  ports: { port: string; to: string[] }[];
  annotations: JsonValue;
  error: string | null;
  elapsed_us: number;
  rewrite?: { length: number; sha256: string };
}
export interface PacketReport {
  packet: number;
  source: string;
  timestamp_ns: number;
  length: number;
  sha256: string;
  rejection: string | null;
  error: string | null;
  input?: PacketSnapshot;
  steps: BlockStep[];
  terminals: string[];
}
export interface Capture {
  name: string | null;
  source: string;
  packets: PacketReport[];
  truncated: boolean;
}
export interface RelayStatus {
  id: string;
  published: number;
  dropped: number;
  queued: number;
  capacity: number;
  failures: number;
}
export interface Status {
  generation: number | null;
  topology: Topology;
  captured: number;
  published: number;
  injected: number;
  filtered: number;
  rejected: number;
  retries: number;
  relays: RelayStatus[];
}
export type StatusResponse =
  { running: true; status: Status } | { running: false; status: null };
