import { useCallback, useState } from "react";
import { Upload, Eye, Clock, Play, Trash2, FileText } from "lucide-react";
import { ipc } from "../ipc";
import { useStore } from "../state/store";
import { open } from "@tauri-apps/plugin-dialog";

export default function DropZone({ fullscreen = false }: { fullscreen?: boolean }) {
  const [dragOver, setDragOver] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const setWorkspace = useStore((s) => s.setWorkspace);
  const mergeWorkspace = useStore((s) => s.mergeWorkspace);
  const hasEvents = useStore((s) => s.events.length > 0);
  const startWatch = useStore((s) => s.startWatch);
  const recentFiles = useStore((s) => s.recentFiles);
  const loadFile = useStore((s) => s.loadFile);
  const removeRecentFile = useStore((s) => s.removeRecentFile);

  const handleRecentClick = async (e: React.MouseEvent, path: string, watchMode: boolean) => {
    e.stopPropagation();
    setBusy(true);
    setError(null);
    try {
      if (watchMode) {
        await startWatch(path);
      } else {
        await loadFile(path);
      }
    } catch (err) {
      setError(`Failed to load: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const handleRemoveRecent = (e: React.MouseEvent, path: string) => {
    e.stopPropagation();
    removeRecentFile(path);
  };

  const onWatchPick = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const selected = await open({
        multiple: false,
        filters: [{
          name: "Log Files",
          extensions: ["log", "jsonl", "json", "txt", "ndjson"]
        }]
      });
      if (selected && typeof selected === "string") {
        setBusy(true);
        setError(null);
        await startWatch(selected);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleFiles = useCallback(
    async (files: FileList | File[]) => {
      setBusy(true);
      setError(null);
      try {
        const arr = Array.from(files);
        for (let i = 0; i < arr.length; i++) {
          const f = arr[i];
          const text = await f.text();
          const res = await ipc.loadText(f.name, text);
          if (i === 0 && !hasEvents) setWorkspace(res.workspace, res.warnings);
          else mergeWorkspace(res.workspace, res.warnings);
        }
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [hasEvents, setWorkspace, mergeWorkspace]
  );

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    if (e.dataTransfer.files.length) handleFiles(e.dataTransfer.files);
  };

  const onPick = () => {
    const input = document.createElement("input");
    input.type = "file";
    input.multiple = true;
    input.accept = ".log,.jsonl,.json,.txt,.ndjson";
    input.onchange = () => {
      if (input.files) handleFiles(input.files);
    };
    input.click();
  };

  const cls = fullscreen
    ? "flex-1 flex items-center justify-center bg-zinc-950 p-6"
    : "p-4";

  return (
    <div className={cls}>
      <div className={fullscreen ? "max-w-xl w-full flex flex-col gap-6" : ""}>
        <div
          onDragOver={(e) => {
            e.preventDefault();
            setDragOver(true);
          }}
          onDragLeave={() => setDragOver(false)}
          onDrop={onDrop}
          onClick={onPick}
          className={`cursor-pointer select-none rounded-lg border-2 border-dashed transition px-10 py-12 text-center w-full ${
            dragOver
              ? "border-emerald-400 bg-emerald-400/5"
              : "border-zinc-700 hover:border-zinc-500 bg-zinc-900/40"
          }`}
        >
          <Upload className="mx-auto mb-3 text-zinc-500" size={28} />
          <div className="text-sm font-medium text-zinc-200">
            {busy ? "Parsing..." : "Drop logs here"}
          </div>
          <div className="text-xs text-zinc-500 mt-1">
            OpenAI-compatible audit · OpenTelemetry GenAI · LiteLLM
          </div>
          <div className="text-xs text-zinc-500 mt-1">
            or click to pick · 100% offline · nothing leaves your machine
          </div>
          <div className="mt-4 flex justify-center">
            <button
              onClick={onWatchPick}
              type="button"
              className="px-3.5 py-1.5 bg-emerald-600/95 hover:bg-emerald-500 text-zinc-100 rounded text-xs font-medium shadow-md transition flex items-center gap-2 border border-emerald-500/20"
            >
              <Eye size={13} className="text-emerald-300" />
              Watch a log file (live updates)
            </button>
          </div>
          {error && (
            <div className="mt-3 text-xs text-rose-400 font-mono">{error}</div>
          )}
        </div>

        {fullscreen && recentFiles.length > 0 && (
          <div className="bg-zinc-900/35 border border-zinc-800/80 rounded-lg p-4 flex flex-col gap-2.5">
            <div className="flex items-center justify-between border-b border-zinc-850 pb-1.5 mb-0.5">
              <span className="text-[10px] font-semibold text-zinc-400 uppercase tracking-wider flex items-center gap-1.5">
                <Clock size={11} className="text-emerald-400" />
                Recent Logs
              </span>
              <span className="text-[9px] text-zinc-500 font-mono">
                Click to load
              </span>
            </div>
            <div className="flex flex-col gap-1.5 max-h-[160px] overflow-y-auto">
              {recentFiles.slice(0, 4).map((file) => (
                <div
                  key={file.path}
                  onClick={(e) => handleRecentClick(e, file.path, file.watched)}
                  className="group flex items-center justify-between px-2.5 py-1.5 bg-zinc-900/50 hover:bg-zinc-950 border border-zinc-800/40 hover:border-zinc-700/60 rounded cursor-pointer transition-colors text-left"
                >
                  <div className="flex items-center gap-2 min-w-0 flex-1 pr-2">
                    <FileText size={12} className="text-zinc-500 shrink-0 group-hover:text-emerald-400 transition-colors" />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="text-xs font-medium text-zinc-300 truncate group-hover:text-zinc-100">
                          {file.name}
                        </span>
                        <span className={`text-[8px] px-1 rounded-sm scale-90 ${
                          file.watched
                            ? "bg-emerald-500/10 text-emerald-400 border border-emerald-500/10"
                            : "bg-blue-500/10 text-blue-400 border border-blue-500/10"
                        }`}>
                          {file.watched ? "Watch" : "Static"}
                        </span>
                      </div>
                      <div className="text-[10px] text-zinc-500 truncate font-mono" title={file.path}>
                        {file.path}
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                    <button
                      onClick={(e) => handleRecentClick(e, file.path, false)}
                      title="Open statically"
                      className="p-1 hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 rounded"
                    >
                      <Play size={11} />
                    </button>
                    <button
                      onClick={(e) => handleRecentClick(e, file.path, true)}
                      title="Watch live"
                      className="p-1 hover:bg-zinc-800 text-zinc-400 hover:text-emerald-400 rounded"
                    >
                      <Eye size={11} />
                    </button>
                    <button
                      onClick={(e) => handleRemoveRecent(e, file.path)}
                      title="Remove from history"
                      className="p-1 hover:bg-zinc-800 text-zinc-500 hover:text-rose-455 rounded"
                    >
                      <Trash2 size={11} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
