import { useEffect } from "react";
import { useStore } from "./state/store";
import TopBar from "./components/TopBar";
import Sidebar from "./components/Sidebar";
import DropZone from "./components/DropZone";
import Timeline from "./components/Timeline";
import Detail from "./components/Detail";
import GraphView from "./components/graph/GraphView";
import Compare from "./components/Compare";
import Metrics from "./components/Metrics";

export default function App() {
  const events = useStore((s) => s.events);
  const view = useStore((s) => s.view);
  const setView = useStore((s) => s.setView);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      const map: Record<string, typeof view> = {
        "1": "timeline",
        "2": "detail",
        "3": "graph",
        "4": "compare",
        "5": "metrics",
      };
      const next = map[e.key];
      if (next) {
        e.preventDefault();
        setView(next);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setView]);

  if (events.length === 0) {
    return (
      <div className="flex flex-col h-full">
        <TopBar />
        <DropZone fullscreen />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <TopBar />
      <div className="flex flex-1 min-h-0">
        <Sidebar />
        <main className="flex-1 min-w-0 min-h-0 overflow-hidden bg-zinc-950">
          {view === "timeline" && <Timeline />}
          {view === "detail" && <Detail />}
          {view === "graph" && <GraphView />}
          {view === "compare" && <Compare />}
          {view === "metrics" && <Metrics />}
        </main>
      </div>
    </div>
  );
}
