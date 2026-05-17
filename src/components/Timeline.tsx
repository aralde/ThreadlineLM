import { useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import Fuse from "fuse.js";
import { useStore } from "../state/store";
import type { LogEvent } from "../types";
import { Check } from "lucide-react";

function statusColor(s: number) {
  if (s >= 500) return "text-rose-400";
  if (s >= 400) return "text-amber-400";
  if (s >= 300) return "text-sky-400";
  if (s >= 200) return "text-emerald-400";
  return "text-zinc-400";
}

function matchSearch(events: LogEvent[], q: string): LogEvent[] {
  if (!q.trim()) return events;

  const tokens = q.match(/\S+/g) ?? [];
  const filters: Array<(e: LogEvent) => boolean> = [];
  const freeText: string[] = [];

  for (const t of tokens) {
    const m = t.match(/^(\w+):(.+)$/);
    if (!m) {
      freeText.push(t);
      continue;
    }
    const [, key, value] = m;
    const v = value.toLowerCase();
    switch (key.toLowerCase()) {
      case "model":
        filters.push((e) => e.model.toLowerCase().includes(v));
        break;
      case "provider":
        filters.push((e) => e.provider.toLowerCase().includes(v));
        break;
      case "status":
        filters.push((e) => String(e.status) === v);
        break;
      case "client":
        filters.push((e) => e.client.toLowerCase().includes(v));
        break;
      default:
        freeText.push(t);
    }
  }

  let res = events.filter((e) => filters.every((f) => f(e)));

  if (freeText.length) {
    const fuse = new Fuse(res, {
      includeScore: false,
      threshold: 0.3,
      keys: [
        "model",
        "provider",
        "client",
        "request.messages.content",
        "response.assistant.content",
      ],
    });
    res = fuse.search(freeText.join(" ")).map((r) => r.item);
  }

  return res;
}

export default function Timeline() {
  const events = useStore((s) => s.events);
  const search = useStore((s) => s.search);
  const selectEvent = useStore((s) => s.selectEvent);
  const compareIds = useStore((s) => s.compareIds);
  const toggleCompare = useStore((s) => s.toggleCompare);

  const filtered = useMemo(() => matchSearch(events, search), [events, search]);

  const parentRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 30,
    overscan: 12,
  });

  return (
    <div className="flex flex-col h-full">
      <div className="grid grid-cols-[140px_70px_1fr_70px_80px_60px_60px] gap-2 px-3 py-1.5 text-[10px] uppercase tracking-wider text-zinc-500 border-b border-zinc-800 bg-zinc-900/60">
        <div>Timestamp</div>
        <div>Provider</div>
        <div>Model</div>
        <div>Status</div>
        <div>Duration</div>
        <div>Tokens</div>
        <div className="text-right">Cmp</div>
      </div>
      <div ref={parentRef} className="flex-1 overflow-auto">
        <div
          style={{ height: rowVirtualizer.getTotalSize(), position: "relative" }}
        >
          {rowVirtualizer.getVirtualItems().map((vi) => {
            const e = filtered[vi.index];
            const inCompare = compareIds.includes(e.id);
            return (
              <div
                key={e.id}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${vi.start}px)`,
                  height: vi.size,
                }}
                className="grid grid-cols-[140px_70px_1fr_70px_80px_60px_60px] gap-2 px-3 items-center text-xs border-b border-zinc-900 hover:bg-zinc-900/60 cursor-pointer"
                onClick={() => selectEvent(e.id)}
              >
                <div className="font-mono text-zinc-400">
                  {new Date(e.ts).toLocaleTimeString()}
                </div>
                <div className="text-zinc-300">{e.provider}</div>
                <div className="truncate text-zinc-200" title={e.model}>
                  {e.model}
                </div>
                <div className={`font-mono ${statusColor(e.status)}`}>
                  {e.status}
                </div>
                <div className="font-mono text-zinc-400">{e.duration_ms} ms</div>
                <div className="font-mono text-zinc-500">
                  {e.response.usage.total_tokens ?? "—"}
                </div>
                <div className="text-right">
                  <button
                    onClick={(ev) => {
                      ev.stopPropagation();
                      toggleCompare(e.id);
                    }}
                    className={`inline-flex items-center justify-center w-5 h-5 rounded border ${
                      inCompare
                        ? "bg-emerald-500/20 border-emerald-500 text-emerald-400"
                        : "border-zinc-700 text-zinc-600 hover:text-zinc-300"
                    }`}
                  >
                    {inCompare && <Check size={10} />}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </div>
      <div className="px-3 py-1 text-[10px] text-zinc-500 border-t border-zinc-800">
        {filtered.length} / {events.length} events
      </div>
    </div>
  );
}
