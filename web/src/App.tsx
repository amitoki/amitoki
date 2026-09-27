import { useEffect, useState } from "react";
import type { ApiClient } from "./api/client";
import { useWorkspace } from "./state/useWorkspace";
import { useStatus } from "./state/useStatus";
import { labels } from "./labels";
import { Navigation, type View } from "./components/Navigation";
import { PacketsView, type PacketSelection } from "./packets/PacketsView";
import { PipelineView } from "./pipeline/PipelineView";
import { OperationsView } from "./operations/OperationsView";

export function App({ client }: { client: ApiClient }) {
  const workspace = useWorkspace(client);
  const observation = useStatus(client, workspace.topology !== null);
  const [view, setView] = useState<View>("packets");
  const [source, setSource] = useState("capture");
  const [stage, setStage] = useState<string | null>(null);
  const [liveTopology, setLiveTopology] = useState(false);
  const [selection, setSelection] = useState<PacketSelection>({
    packet: null,
    step: 0,
    page: 0,
  });
  const status = observation.response?.running
    ? observation.response.status
    : null;
  const active = liveTopology && status !== null;
  const topology = workspace.topology;
  const pipeline = active ? status.topology : topology;
  const error = workspace.error ?? observation.error;
  const running = status !== null;
  useEffect(() => {
    if (!running) setLiveTopology(false);
  }, [running]);

  function selectView(next: View) {
    if (view === "pipeline" && next === "packets") {
      const packet =
        workspace.capture.packets.find(
          (entry) => entry.packet === selection.packet,
        ) ?? workspace.capture.packets[0];
      const selectedStage =
        pipeline?.stages.find((entry) => entry.id === stage) ??
        pipeline?.stages[0];
      setSelection({
        ...selection,
        step: Math.max(
          0,
          packet?.steps.findIndex((step) => step.block === selectedStage?.id) ??
            0,
        ),
      });
    }
    setView(next);
  }
  async function upload(file: File) {
    const capture = await workspace.upload(file, source);
    if (capture)
      setSelection({
        packet: capture.packets[0]?.packet ?? null,
        step: 0,
        page: 0,
      });
  }
  return (
    <div id="amitoki-design" className="min-h-screen w-full">
      <header className="am-top flex items-center gap-3.5 border-b border-line bg-panel px-5 py-1.5 max-compact:gap-2 max-compact:px-3.5">
        <div className="am-brand flex items-center gap-[9px] text-[19px] font-semibold tracking-[-0.6px] [&_svg]:size-[21px] [&_svg]:text-accent">
          <svg
            className="fill-none stroke-current stroke-[1.5]"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path d="M12 8v5M5 16v-3h14v3" />
            <rect x="9" y="2" width="6" height="6" rx="1" />
            <rect x="2" y="16" width="6" height="6" rx="1" />
            <rect x="16" y="16" width="6" height="6" rx="1" />
          </svg>
          amitoki
        </div>
        <div className="am-local flex min-w-0 items-center gap-[7px] border-l border-line pl-3.5 max-compact:pl-2 max-compact:text-[11px] max-compact:[&_.am-caption]:hidden">
          <span
            id="connection-dot"
            className={`am-dot size-1.5 shrink-0 rounded-full bg-good [&.am-offline]:bg-muted ${running ? "" : "am-offline"}`}
          />
          <span
            id="node-name"
            className="max-w-[160px] truncate max-compact:max-w-[100px]"
          >
            {topology?.node_id ?? "—"}
          </span>
          <span className="am-caption">LOCAL</span>
        </div>
        <span
          className="am-top-note ml-auto text-[12px] text-muted max-compact:whitespace-nowrap max-compact:text-[11px]"
          id="connection-state"
        >
          {running ? labels.running : labels.unknown}
        </span>
      </header>
      <div className="am-layout grid grid-cols-[146px_minmax(0,1fr)] max-wide:grid-cols-[128px_minmax(0,1fr)] max-compact:block">
        <aside className="am-sidebar border-r border-line bg-panel px-2.5 py-5 max-compact:border-r-0 max-compact:border-b max-compact:p-2">
          <Navigation view={view} onSelect={selectView} />
          <div className="am-branch mx-2.5 mt-7 text-[11px] text-muted max-compact:hidden [&_strong]:my-1 [&_strong]:block [&_strong]:font-medium [&_strong]:text-ink [&_strong]:wrap-anywhere">
            <strong id="channel-name">{topology?.channel ?? "—"}</strong>
            <span id="pipeline-count">
              {topology
                ? `${topology.stages.length} Stage / ${topology.relays.length} Relay`
                : ""}
            </span>
          </div>
        </aside>
        <main className="am-main w-full min-w-0 max-w-[1600px] p-[22px] max-wide:p-4 max-compact:p-3.5">
          <div
            id="message"
            className="mb-4 rounded-md border border-line bg-panel p-3 text-bad wrap-anywhere"
            role="alert"
            hidden={!error}
          >
            {error}
          </div>
          <div id="am-view" aria-live="polite">
            {topology && (
              <>
                {view === "packets" && (
                  <PacketsView
                    topology={topology}
                    capture={workspace.capture}
                    busy={workspace.busy}
                    source={source}
                    selection={selection}
                    onSource={setSource}
                    onSelection={setSelection}
                    onUpload={(file) => {
                      void upload(file);
                    }}
                  />
                )}
                {view === "pipeline" && pipeline && (
                  <PipelineView
                    topology={pipeline}
                    active={active}
                    running={running}
                    stage={stage}
                    onActive={setLiveTopology}
                    onStage={setStage}
                    onPackets={() => selectView("packets")}
                  />
                )}
                {view === "operations" && (
                  <OperationsView
                    topology={status?.topology ?? topology}
                    status={status}
                    events={observation.events}
                  />
                )}
              </>
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
