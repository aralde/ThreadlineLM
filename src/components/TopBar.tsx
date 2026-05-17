import { useStore } from "../state/store";
import {
  Activity,
  GitBranch,
  ListOrdered,
  Moon,
  Search,
  SplitSquareHorizontal,
  Sun,
  Trash2,
  FileText,
} from "lucide-react";

const tabs = [
  { id: "timeline", label: "Timeline", icon: ListOrdered },
  { id: "detail", label: "Detail", icon: FileText },
  { id: "graph", label: "Graph", icon: GitBranch },
  { id: "compare", label: "Compare", icon: SplitSquareHorizontal },
  { id: "metrics", label: "Metrics", icon: Activity },
] as const;

export default function TopBar() {
  const view = useStore((s) => s.view);
  const setView = useStore((s) => s.setView);
  const search = useStore((s) => s.search);
  const setSearch = useStore((s) => s.setSearch);
  const reset = useStore((s) => s.reset);
  const events = useStore((s) => s.events);
  const theme = useStore((s) => s.theme);
  const toggleTheme = useStore((s) => s.toggleTheme);

  return (
    <header className="flex items-center gap-3 px-3 h-10 border-b border-zinc-800 bg-zinc-900">
      <div className="flex items-center gap-2 font-semibold tracking-tight">
        <span className="inline-block w-2 h-2 rounded-full bg-emerald-400" />
        ThreadlineLM
      </div>

      <nav className="flex items-center gap-1 ml-2">
        {tabs.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => setView(id)}
            disabled={events.length === 0}
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded text-xs transition ${
              view === id
                ? "bg-zinc-800 text-zinc-100"
                : "text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800/60"
            } disabled:opacity-40 disabled:hover:bg-transparent`}
          >
            <Icon size={13} />
            {label}
          </button>
        ))}
      </nav>

      <div className="flex-1" />

      {events.length > 0 && (
        <>
          <div className="relative">
            <Search
              size={13}
              className="absolute left-2 top-1/2 -translate-y-1/2 text-zinc-500"
            />
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search messages, model:, provider:, status:"
              className="w-80 pl-7 pr-2 py-1 text-xs bg-zinc-950 border border-zinc-800 rounded focus:outline-none focus:border-zinc-600"
            />
          </div>
          <button
            onClick={reset}
            title="Clear workspace"
            className="p-1 text-zinc-500 hover:text-rose-400"
          >
            <Trash2 size={14} />
          </button>
        </>
      )}
      <button
        onClick={toggleTheme}
        title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
        className="p-1 text-zinc-500 hover:text-zinc-100"
      >
        {theme === "dark" ? <Sun size={14} /> : <Moon size={14} />}
      </button>
    </header>
  );
}
