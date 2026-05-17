import { useCallback, useState } from "react";
import { Upload } from "lucide-react";
import { ipc } from "../ipc";
import { useStore } from "../state/store";

export default function DropZone({ fullscreen = false }: { fullscreen?: boolean }) {
  const [dragOver, setDragOver] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const setWorkspace = useStore((s) => s.setWorkspace);
  const mergeWorkspace = useStore((s) => s.mergeWorkspace);
  const hasEvents = useStore((s) => s.events.length > 0);

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
    ? "flex-1 flex items-center justify-center bg-zinc-950"
    : "p-4";

  return (
    <div className={cls}>
      <div
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={onDrop}
        onClick={onPick}
        className={`cursor-pointer select-none rounded-lg border-2 border-dashed transition px-10 py-12 text-center max-w-xl w-full ${
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
          operatorlm · OpenTelemetry GenAI · LiteLLM
        </div>
        <div className="text-xs text-zinc-500 mt-1">
          or click to pick · 100% offline · nothing leaves your machine
        </div>
        {error && (
          <div className="mt-3 text-xs text-rose-400 font-mono">{error}</div>
        )}
      </div>
    </div>
  );
}
