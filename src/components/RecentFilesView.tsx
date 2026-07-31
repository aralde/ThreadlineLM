import { useState } from "react";
import { useStore, RecentFile } from "../state/store";
import { Clock, Play, Eye, Trash2, FileText, AlertCircle, RefreshCw } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";

export default function RecentFilesView() {
  const recentFiles = useStore((s) => s.recentFiles);
  const removeRecentFile = useStore((s) => s.removeRecentFile);
  const clearRecentFiles = useStore((s) => s.clearRecentFiles);
  const loadFile = useStore((s) => s.loadFile);
  const startWatch = useStore((s) => s.startWatch);
  const setView = useStore((s) => s.setView);

  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const formatTimeAgo = (timestamp: number): string => {
    const diff = Date.now() - timestamp;
    const secs = Math.floor(diff / 1000);
    if (secs < 60) return "Just now";
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  };

  const handleOpenRecent = async (file: RecentFile, mode: "static" | "watch") => {
    setLoadingPath(file.path);
    setErrorMsg(null);
    try {
      if (mode === "watch") {
        await startWatch(file.path);
      } else {
        await loadFile(file.path);
      }
      setView("timeline"); // Switch to timeline view on success
    } catch (err) {
      setErrorMsg(`Failed to load "${file.name}": ${String(err)}`);
    } finally {
      setLoadingPath(null);
    }
  };

  const handleBrowseNew = async (mode: "static" | "watch") => {
    setErrorMsg(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [{
          name: "Log Files",
          extensions: ["log", "jsonl", "json", "txt", "ndjson"]
        }]
      });
      if (selected && typeof selected === "string") {
        setLoadingPath(selected);
        if (mode === "watch") {
          await startWatch(selected);
        } else {
          await loadFile(selected);
        }
        setView("timeline");
      }
    } catch (err) {
      setErrorMsg(`Error opening file: ${String(err)}`);
    } finally {
      setLoadingPath(null);
    }
  };

  return (
    <div className="flex-1 overflow-y-auto bg-zinc-950 p-6 flex justify-center">
      <div className="w-full max-w-4xl flex flex-col gap-6">
        
        {/* Header */}
        <div className="flex items-center justify-between border-b border-zinc-800 pb-4">
          <div>
            <h1 className="text-xl font-semibold text-zinc-100 flex items-center gap-2">
              <Clock size={20} className="text-emerald-400" />
              Recent Files
            </h1>
            <p className="text-xs text-zinc-400 mt-1">
              Quickly reload files you've opened in the past or open a new one.
            </p>
          </div>
          {recentFiles.length > 0 && (
            <button
              onClick={clearRecentFiles}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-900 hover:bg-rose-950/40 text-zinc-400 hover:text-rose-400 border border-zinc-800 hover:border-rose-900/50 rounded text-xs transition font-medium"
            >
              <Trash2 size={13} />
              Clear History
            </button>
          )}
        </div>

        {/* Error notification */}
        {errorMsg && (
          <div className="flex items-start gap-2.5 p-3.5 bg-rose-500/10 border border-rose-500/20 text-rose-300 rounded text-xs font-mono">
            <AlertCircle size={14} className="mt-0.5 shrink-0" />
            <div className="flex-1">{errorMsg}</div>
            <button 
              onClick={() => setErrorMsg(null)}
              className="hover:text-rose-100 font-semibold cursor-pointer text-[10px]"
            >
              ✕
            </button>
          </div>
        )}

        {/* Main Content Area */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          
          {/* Recent Files List */}
          <div className="md:col-span-2 flex flex-col gap-2">
            <h2 className="text-xs font-semibold text-zinc-500 uppercase tracking-wider mb-1">
              File History
            </h2>
            
            {recentFiles.length === 0 ? (
              <div className="flex flex-col items-center justify-center p-12 border border-zinc-800 border-dashed rounded-lg bg-zinc-900/20 text-center">
                <FileText size={32} className="text-zinc-600 mb-3" />
                <p className="text-sm text-zinc-400 font-medium">No recent files found</p>
                <p className="text-xs text-zinc-500 mt-1">
                  Once you watch or load files, they will appear here.
                </p>
              </div>
            ) : (
              <div className="flex flex-col gap-2.5">
                {recentFiles.map((file) => {
                  const isLoading = loadingPath === file.path;
                  return (
                    <div
                      key={file.path}
                      className="group flex items-center justify-between p-3.5 bg-zinc-900/40 hover:bg-zinc-900/80 border border-zinc-800 hover:border-zinc-700 rounded-lg transition-all duration-200"
                    >
                      <div className="flex items-center gap-3 min-w-0 flex-1 pr-4">
                        <div className="p-2 bg-zinc-800/60 rounded text-zinc-400 group-hover:text-emerald-400 transition-colors">
                          <FileText size={16} />
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <span className="font-medium text-sm text-zinc-200 truncate group-hover:text-zinc-100">
                              {file.name}
                            </span>
                            <span className={`text-[9px] px-1.5 py-0.5 rounded font-medium ${
                              file.watched 
                                ? "bg-emerald-500/10 text-emerald-400 border border-emerald-500/20" 
                                : "bg-blue-500/10 text-blue-400 border border-blue-500/20"
                            }`}>
                              {file.watched ? "Watch" : "Static"}
                            </span>
                          </div>
                          <div 
                            className="text-xs text-zinc-500 truncate font-mono mt-0.5" 
                            title={file.path}
                          >
                            {file.path}
                          </div>
                        </div>
                      </div>

                      <div className="flex items-center gap-2 shrink-0">
                        <span className="text-[10px] text-zinc-500 mr-2">
                          {formatTimeAgo(file.timestamp)}
                        </span>
                        
                        {isLoading ? (
                          <div className="flex items-center justify-center w-8 h-8 rounded bg-zinc-800 text-emerald-400">
                            <RefreshCw size={13} className="animate-spin" />
                          </div>
                        ) : (
                          <>
                            <button
                              onClick={() => handleOpenRecent(file, "static")}
                              disabled={loadingPath !== null}
                              title="Open statically (read-once)"
                              className="p-1.5 hover:bg-zinc-800 text-zinc-400 hover:text-zinc-100 rounded transition disabled:opacity-40"
                            >
                              <Play size={13} />
                            </button>
                            <button
                              onClick={() => handleOpenRecent(file, "watch")}
                              disabled={loadingPath !== null}
                              title="Open in Watch mode (live updates)"
                              className="p-1.5 hover:bg-zinc-800 text-zinc-400 hover:text-emerald-400 rounded transition disabled:opacity-40"
                            >
                              <Eye size={13} />
                            </button>
                            <button
                              onClick={() => removeRecentFile(file.path)}
                              title="Remove from history"
                              className="p-1.5 hover:bg-zinc-800 text-zinc-500 hover:text-rose-400 rounded transition"
                            >
                              <Trash2 size={13} />
                            </button>
                          </>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
          
          {/* Quick Actions Panel */}
          <div className="flex flex-col gap-4">
            <h2 className="text-xs font-semibold text-zinc-500 uppercase tracking-wider">
              Quick Actions
            </h2>
            
            <div className="flex flex-col gap-3 p-4 bg-zinc-900/30 border border-zinc-800 rounded-lg">
              <p className="text-xs text-zinc-400 leading-relaxed mb-1">
                Open any new LLM trace log file using either the Static (read-once) parser or start a Live Watch session to stream updates.
              </p>
              
              <button
                onClick={() => handleBrowseNew("static")}
                disabled={loadingPath !== null}
                className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-100 rounded text-xs font-medium transition shadow-sm border border-zinc-700/35 disabled:opacity-55"
              >
                <Play size={13} />
                Open log file (Static)
              </button>

              <button
                onClick={() => handleBrowseNew("watch")}
                disabled={loadingPath !== null}
                className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-zinc-100 rounded text-xs font-medium transition shadow-md border border-emerald-500/20 disabled:opacity-55"
              >
                <Eye size={13} className="text-emerald-300" />
                Watch log file (Live)
              </button>
            </div>
          </div>

        </div>

      </div>
    </div>
  );
}
