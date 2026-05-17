import { useMemo } from "react";
import { useStore } from "../state/store";

function pct(arr: number[], p: number) {
  if (!arr.length) return 0;
  const s = [...arr].sort((a, b) => a - b);
  const i = Math.min(s.length - 1, Math.floor((p / 100) * s.length));
  return s[i];
}

const DEFAULT_PRICING: Record<string, [number, number]> = {
  "gpt-4o": [2.5, 10],
  "gpt-4o-mini": [0.15, 0.6],
  "claude-3-5-sonnet": [3, 15],
  "claude-3-5-haiku": [0.8, 4],
  "gemini-3-flash-preview": [0.075, 0.3],
  "llama-4-scout-17b-16e-instruct": [0.11, 0.34],
};

function priceFor(model: string): [number, number] | null {
  const key = model.toLowerCase().replace(/^[^/]+\//, "");
  for (const [k, v] of Object.entries(DEFAULT_PRICING)) {
    if (key.includes(k)) return v;
  }
  return null;
}

export default function Metrics() {
  const events = useStore((s) => s.events);

  const stats = useMemo(() => {
    const durations = events.map((e) => e.duration_ms);
    let promptTok = 0,
      completionTok = 0,
      cost = 0,
      withTokens = 0,
      errors = 0;
    const byProvider = new Map<string, number>();
    const byModel = new Map<string, number>();

    for (const e of events) {
      if (e.status >= 400) errors++;
      byProvider.set(e.provider, (byProvider.get(e.provider) ?? 0) + 1);
      byModel.set(e.model, (byModel.get(e.model) ?? 0) + 1);
      const u = e.response.usage;
      if (u.prompt_tokens || u.completion_tokens) {
        withTokens++;
        promptTok += u.prompt_tokens ?? 0;
        completionTok += u.completion_tokens ?? 0;
        const p = priceFor(e.model);
        if (p) {
          cost +=
            ((u.prompt_tokens ?? 0) / 1_000_000) * p[0] +
            ((u.completion_tokens ?? 0) / 1_000_000) * p[1];
        }
      }
    }

    return {
      total: events.length,
      errors,
      p50: pct(durations, 50),
      p95: pct(durations, 95),
      p99: pct(durations, 99),
      max: Math.max(0, ...durations),
      promptTok,
      completionTok,
      withTokens,
      cost,
      byProvider: [...byProvider.entries()].sort((a, b) => b[1] - a[1]),
      byModel: [...byModel.entries()].sort((a, b) => b[1] - a[1]),
    };
  }, [events]);

  const Stat = ({ label, value, sub }: { label: string; value: string; sub?: string }) => (
    <div className="border border-zinc-800 rounded p-3 bg-zinc-900/40">
      <div className="text-[10px] uppercase tracking-wider text-zinc-500">
        {label}
      </div>
      <div className="text-xl font-mono text-zinc-100 mt-0.5">{value}</div>
      {sub && <div className="text-[10px] text-zinc-500 mt-0.5">{sub}</div>}
    </div>
  );

  return (
    <div className="p-4 space-y-4 overflow-auto h-full">
      <div className="grid grid-cols-4 gap-3">
        <Stat label="Requests" value={String(stats.total)} sub={`${stats.errors} errors`} />
        <Stat label="Latency p50 / p95" value={`${stats.p50} / ${stats.p95} ms`} sub={`p99 ${stats.p99} ms · max ${stats.max} ms`} />
        <Stat
          label="Tokens (in / out)"
          value={`${stats.promptTok.toLocaleString()} / ${stats.completionTok.toLocaleString()}`}
          sub={`${stats.withTokens}/${stats.total} req with usage`}
        />
        <Stat
          label="Estimated cost"
          value={`$${stats.cost.toFixed(4)}`}
          sub="based on built-in pricing table"
        />
      </div>

      <div className="grid grid-cols-2 gap-3">
        <div className="border border-zinc-800 rounded p-3 bg-zinc-900/40">
          <div className="text-[10px] uppercase tracking-wider text-zinc-500 mb-2">
            By provider
          </div>
          <table className="w-full text-xs">
            <tbody>
              {stats.byProvider.map(([p, n]) => (
                <tr key={p} className="border-t border-zinc-800/60">
                  <td className="py-1 text-zinc-300">{p}</td>
                  <td className="py-1 text-right font-mono text-zinc-400">{n}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="border border-zinc-800 rounded p-3 bg-zinc-900/40">
          <div className="text-[10px] uppercase tracking-wider text-zinc-500 mb-2">
            By model
          </div>
          <table className="w-full text-xs">
            <tbody>
              {stats.byModel.map(([m, n]) => (
                <tr key={m} className="border-t border-zinc-800/60">
                  <td className="py-1 text-zinc-300 truncate max-w-[260px]" title={m}>
                    {m}
                  </td>
                  <td className="py-1 text-right font-mono text-zinc-400">{n}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
