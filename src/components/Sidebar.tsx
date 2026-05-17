import { useState } from "react";
import { useStore } from "../state/store";
import { FileText, Layers, Plus } from "lucide-react";
import DropZone from "./DropZone";

export default function Sidebar() {
  const [tab, setTab] = useState<"sessions" | "files">("sessions");
  const [showAdd, setShowAdd] = useState(false);
  const files = useStore((s) => s.files);
  const sessions = useStore((s) => s.sessions);
  const selectedSessionId = useStore((s) => s.selectedSessionId);
  const selectSession = useStore((s) => s.selectSession);
  const setView = useStore((s) => s.setView);

  return (
    <aside className="w-64 shrink-0 flex flex-col border-r border-zinc-800 bg-zinc-900/40">
      <div className="flex border-b border-zinc-800">
        <button
          onClick={() => setTab("sessions")}
          className={`flex-1 px-2 py-1.5 text-xs flex items-center justify-center gap-1.5 ${
            tab === "sessions" ? "text-zinc-100 bg-zinc-800/50" : "text-zinc-400"
          }`}
        >
          <Layers size={12} /> Sessions ({sessions.length})
        </button>
        <button
          onClick={() => setTab("files")}
          className={`flex-1 px-2 py-1.5 text-xs flex items-center justify-center gap-1.5 ${
            tab === "files" ? "text-zinc-100 bg-zinc-800/50" : "text-zinc-400"
          }`}
        >
          <FileText size={12} /> Files ({files.length})
        </button>
      </div>

      <div className="flex-1 overflow-auto">
        {tab === "sessions" &&
          sessions.map((s) => {
            const dur =
              new Date(s.ended_at).getTime() - new Date(s.started_at).getTime();
            return (
              <button
                key={s.id}
                onClick={() => {
                  selectSession(s.id);
                  setView("graph");
                }}
                className={`w-full text-left px-3 py-2 border-b border-zinc-800/60 hover:bg-zinc-800/40 ${
                  selectedSessionId === s.id ? "bg-zinc-800/60" : ""
                }`}
              >
                <div className="text-xs font-mono text-zinc-300 truncate">
                  {s.client}
                </div>
                <div className="text-[10px] text-zinc-500 mt-0.5 flex gap-2">
                  <span>{s.event_ids.length} req</span>
                  <span>·</span>
                  <span>{Math.round(dur / 1000)}s</span>
                  <span>·</span>
                  <span>{new Date(s.started_at).toLocaleTimeString()}</span>
                </div>
              </button>
            );
          })}

        {tab === "files" &&
          files.map((f) => (
            <div
              key={f.path}
              className="px-3 py-2 border-b border-zinc-800/60"
            >
              <div className="text-xs text-zinc-200 truncate">{f.name}</div>
              <div className="text-[10px] text-zinc-500 mt-0.5">
                {f.events} events · {(f.bytes / 1024).toFixed(1)} KB
              </div>
            </div>
          ))}
      </div>

      <button
        onClick={() => setShowAdd((v) => !v)}
        className="flex items-center gap-1.5 px-3 py-1.5 text-xs text-zinc-400 border-t border-zinc-800 hover:text-zinc-100 hover:bg-zinc-800/40"
      >
        <Plus size={12} /> Add files
      </button>
      {showAdd && <DropZone />}
    </aside>
  );
}
