import { useMemo } from "react";
import DiffMatchPatch from "diff-match-patch";
import { useStore } from "../state/store";
import type { LogEvent } from "../types";

const dmp = new DiffMatchPatch();

function eventToText(e: LogEvent) {
  const parts: string[] = [];
  parts.push(`# ${e.provider}/${e.model}  status=${e.status}  ${e.duration_ms}ms`);
  parts.push("");
  parts.push("## REQUEST");
  for (const m of e.request.messages) {
    parts.push(`[${m.role}] ${m.content ?? ""}`);
    if (m.tool_calls) {
      for (const tc of m.tool_calls)
        parts.push(`  -> tool ${tc.name}(${tc.arguments})`);
    }
  }
  parts.push("");
  parts.push("## RESPONSE");
  const a = e.response.assistant;
  if (a) {
    parts.push(`[${a.role}] ${a.content ?? ""}`);
    if (a.tool_calls) {
      for (const tc of a.tool_calls)
        parts.push(`  -> tool ${tc.name}(${tc.arguments})`);
    }
  }
  return parts.join("\n");
}

function diffHtml(a: string, b: string): string {
  const diffs = dmp.diff_main(a, b);
  dmp.diff_cleanupSemantic(diffs);
  return diffs
    .map(([op, text]) => {
      const safe = text.replace(/[&<>]/g, (c) =>
        c === "&" ? "&amp;" : c === "<" ? "&lt;" : "&gt;"
      );
      if (op === 0) return `<span class="text-zinc-400">${safe}</span>`;
      if (op === -1)
        return `<span class="bg-rose-900/40 text-rose-200 line-through">${safe}</span>`;
      return `<span class="bg-emerald-900/40 text-emerald-200">${safe}</span>`;
    })
    .join("");
}

export default function Compare() {
  const events = useStore((s) => s.events);
  const [aId, bId] = useStore((s) => s.compareIds);

  const a = useMemo(() => events.find((e) => e.id === aId) ?? null, [events, aId]);
  const b = useMemo(() => events.find((e) => e.id === bId) ?? null, [events, bId]);

  if (!a || !b) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-zinc-500">
        Pick two events in the Timeline (Cmp column) to compare.
      </div>
    );
  }

  const ta = eventToText(a);
  const tb = eventToText(b);

  return (
    <div className="grid grid-cols-2 h-full">
      <div className="overflow-auto border-r border-zinc-800">
        <div className="px-3 py-1.5 text-xs text-zinc-300 bg-zinc-900 border-b border-zinc-800">
          {a.provider}/{a.model} · {a.id.slice(0, 8)}
        </div>
        <pre
          className="p-3 text-[11px] font-mono whitespace-pre-wrap leading-snug"
          dangerouslySetInnerHTML={{ __html: diffHtml(ta, tb) }}
        />
      </div>
      <div className="overflow-auto">
        <div className="px-3 py-1.5 text-xs text-zinc-300 bg-zinc-900 border-b border-zinc-800">
          {b.provider}/{b.model} · {b.id.slice(0, 8)}
        </div>
        <pre
          className="p-3 text-[11px] font-mono whitespace-pre-wrap leading-snug"
          dangerouslySetInnerHTML={{ __html: diffHtml(tb, ta) }}
        />
      </div>
    </div>
  );
}
