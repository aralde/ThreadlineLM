import { useCallback, useEffect, useMemo, useState } from "react";
import ReactFlow, {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlowProvider,
  useReactFlow,
  type Edge,
  type Node,
} from "reactflow";
import dagre from "dagre";
import {
  ChevronDown,
  ChevronRight,
  Cog,
  MessageSquare,
  Sparkles,
  User,
  Wrench,
  X,
} from "lucide-react";
import { useStore } from "../../state/store";
import type { ConversationTurn, LogEvent, Session } from "../../types";
import Detail from "../Detail";

const NODE_W = 360;
const COLLAPSED_H = 56;
const EXPANDED_H = 220;

type Kind = "system" | "user" | "assistant" | "tool_call" | "tool_result";

interface CardData {
  kind: Kind;
  title: string;
  preview: string;
  body: string;
  eventId: string;
  collapsed: boolean;
  height: number;
  onToggle: () => void;
  onJump: () => void;
}

function colors(kind: Kind) {
  switch (kind) {
    case "system":
      return { border: "border-zinc-700", bg: "bg-zinc-900/80", text: "text-zinc-300", accent: "text-zinc-500", handle: "#52525b" };
    case "user":
      return { border: "border-sky-700", bg: "bg-sky-950/80", text: "text-sky-100", accent: "text-sky-400", handle: "#0369a1" };
    case "assistant":
      return { border: "border-emerald-700", bg: "bg-emerald-950/80", text: "text-emerald-100", accent: "text-emerald-400", handle: "#047857" };
    case "tool_call":
      return { border: "border-violet-700", bg: "bg-violet-950/80", text: "text-violet-100", accent: "text-violet-400", handle: "#6d28d9" };
    case "tool_result":
      return { border: "border-amber-700", bg: "bg-amber-950/80", text: "text-amber-100", accent: "text-amber-400", handle: "#b45309" };
  }
}

function Icon({ kind, size = 13 }: { kind: Kind; size?: number }) {
  switch (kind) {
    case "system":
      return <Cog size={size} />;
    case "user":
      return <User size={size} />;
    case "assistant":
      return <Sparkles size={size} />;
    case "tool_call":
      return <Wrench size={size} />;
    case "tool_result":
      return <MessageSquare size={size} />;
  }
}

function CardNode({ data }: { data: CardData }) {
  const c = colors(data.kind);
  return (
    <div
      className={`relative rounded border ${c.border} ${c.bg} ${c.text} shadow-sm cursor-pointer hover:brightness-110`}
      style={{ width: NODE_W, height: data.height }}
      onClick={data.onJump}
    >
      <Handle
        type="target"
        position={Position.Top}
        style={{ background: c.handle, width: 8, height: 8, border: "none" }}
      />
      <div className="flex items-center gap-2 px-2 py-1.5 border-b border-white/5">
        <button
          onClick={(e) => {
            e.stopPropagation();
            data.onToggle();
          }}
          className={`${c.accent} hover:opacity-100 opacity-80 nodrag`}
          title={data.collapsed ? "Expand" : "Collapse"}
        >
          {data.collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
        </button>
        <span className={c.accent}>
          <Icon kind={data.kind} />
        </span>
        <div className="text-[10px] uppercase tracking-wider opacity-80 truncate">
          {data.title}
        </div>
      </div>
      {data.collapsed ? (
        <div className="px-2 py-1.5 text-[11px] font-mono truncate opacity-80">
          {data.preview || "(empty)"}
        </div>
      ) : (
        <div
          className="px-2 py-1.5 text-[11px] font-mono whitespace-pre-wrap overflow-hidden"
          style={{ maxHeight: EXPANDED_H - 40 }}
        >
          {data.body || "(empty)"}
        </div>
      )}
      <Handle
        type="source"
        position={Position.Bottom}
        style={{ background: c.handle, width: 8, height: 8, border: "none" }}
      />
    </div>
  );
}

const nodeTypes = { card: CardNode };

interface TurnView {
  index: number;
  kind: Kind;
  title: string;
  preview: string;
  body: string;
  eventId: string;
  /** Detail-panel target: "msg:<i>", "resp", or "tc:<id>". */
  focusTarget: string;
}

function previewOf(s: string, max = 80): string {
  const oneLine = s.replace(/\s+/g, " ").trim();
  return oneLine.length > max ? oneLine.slice(0, max) + "…" : oneLine;
}

function expandTurns(
  conv: ConversationTurn[],
  winnerEvent: LogEvent | null
): TurnView[] {
  // The conversation array mirrors the winning event's `request.messages`,
  // with the final assistant response appended at the end (see session.rs).
  // Map each conv entry back to its bubble in Detail: "msg:<i>" for the
  // request messages and "resp" for that trailing response bubble.
  const hasResp = !!winnerEvent?.response.assistant;
  const respIndex = hasResp ? conv.length - 1 : -1;

  const out: TurnView[] = [];
  let i = 0;
  conv.forEach((t, convIdx) => {
    const bubbleId = convIdx === respIndex ? "resp" : `msg:${convIdx}`;

    if (t.role === "assistant" && t.tool_calls && t.tool_calls.length > 0) {
      if (t.content && t.content.trim().length > 0) {
        out.push({
          index: i++,
          kind: "assistant",
          title: "ASSISTANT · thinking",
          preview: previewOf(t.content),
          body: t.content,
          eventId: t.event_id,
          focusTarget: bubbleId,
        });
      }
      for (const tc of t.tool_calls) {
        const args =
          tc.arguments && tc.arguments.length > 200
            ? tc.arguments.slice(0, 200) + "…"
            : tc.arguments;
        out.push({
          index: i++,
          kind: "tool_call",
          title: `TOOL CALL · ${tc.name}`,
          preview: `${tc.name}(${previewOf(tc.arguments, 60)})`,
          body: `${tc.name}(${args})`,
          eventId: t.event_id,
          focusTarget: tc.id ? `tc:${tc.id}` : bubbleId,
        });
      }
      return;
    }
    const kind: Kind =
      t.role === "system"
        ? "system"
        : t.role === "user"
        ? "user"
        : t.role === "tool"
        ? "tool_result"
        : "assistant";
    const title =
      kind === "tool_result"
        ? "TOOL RESULT"
        : kind === "assistant"
        ? "ASSISTANT"
        : kind.toUpperCase();
    out.push({
      index: i++,
      kind,
      title,
      preview: previewOf(t.content),
      body: t.content,
      eventId: t.event_id,
      focusTarget: bubbleId,
    });
  });
  return out;
}

function buildGraph(
  turns: TurnView[],
  collapsed: Set<number>,
  toggle: (idx: number) => void,
  jump: (eventId: string, focusTarget: string) => void,
  edgeColor: string
): { nodes: Node<CardData>[]; edges: Edge[] } {
  const nodes: Node<CardData>[] = turns.map((t) => {
    const isCollapsed = collapsed.has(t.index);
    const height = isCollapsed ? COLLAPSED_H : EXPANDED_H;
    return {
      id: `t:${t.index}`,
      type: "card",
      data: {
        kind: t.kind,
        title: t.title,
        preview: t.preview,
        body: t.body,
        eventId: t.eventId,
        collapsed: isCollapsed,
        height,
        onToggle: () => toggle(t.index),
        onJump: () => jump(t.eventId, t.focusTarget),
      },
      position: { x: 0, y: 0 },
      style: { width: NODE_W, height },
    };
  });

  const edges: Edge[] = [];
  for (let i = 1; i < turns.length; i++) {
    edges.push({
      id: `e:${i - 1}-${i}`,
      source: `t:${i - 1}`,
      target: `t:${i}`,
      type: "smoothstep",
      style: { stroke: edgeColor, strokeWidth: 1.5 },
      animated: true,
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: edgeColor,
        width: 18,
        height: 18,
      },
    });
  }

  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: "TB", nodesep: 32, ranksep: 48, marginx: 20, marginy: 20 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const n of nodes) {
    g.setNode(n.id, { width: NODE_W, height: n.data.height });
  }
  for (const e of edges) g.setEdge(e.source, e.target);
  dagre.layout(g);

  for (const n of nodes) {
    const p = g.node(n.id);
    n.position = { x: p.x - NODE_W / 2, y: p.y - n.data.height / 2 };
  }
  return { nodes, edges };
}

function GraphInner({
  nodes,
  edges,
  showDetail,
  expandAll,
  collapseAll,
  openDetail,
  turnsCount,
  bgDotsColor,
}: {
  nodes: Node<CardData>[];
  edges: Edge[];
  showDetail: boolean;
  expandAll: () => void;
  collapseAll: () => void;
  openDetail: () => void;
  turnsCount: number;
  bgDotsColor: string;
}) {
  const { fitView } = useReactFlow();

  useEffect(() => {
    const t = setTimeout(() => fitView({ padding: 0.15, duration: 250 }), 50);
    return () => clearTimeout(t);
  }, [showDetail, nodes.length, fitView]);

  return (
    <>
      <div className="absolute top-2 left-2 z-10 flex items-center gap-1 bg-zinc-900/90 border border-zinc-800 rounded p-1 text-[11px]">
        <button
          onClick={expandAll}
          className="px-2 py-0.5 text-zinc-300 hover:bg-zinc-800 rounded"
        >
          Expand all
        </button>
        <button
          onClick={collapseAll}
          className="px-2 py-0.5 text-zinc-300 hover:bg-zinc-800 rounded"
        >
          Collapse all
        </button>
        <span className="px-2 py-0.5 text-zinc-500">{turnsCount} turns</span>
        {!showDetail && (
          <button
            onClick={openDetail}
            className="px-2 py-0.5 text-zinc-300 hover:bg-zinc-800 rounded"
          >
            Show detail →
          </button>
        )}
      </div>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.15 }}
        nodesDraggable={false}
        nodesConnectable={false}
        proOptions={{ hideAttribution: true }}
      >
        <Background gap={16} color={bgDotsColor} />
        <MiniMap pannable zoomable className="!bg-zinc-900" />
        <Controls className="!bg-zinc-900 !border-zinc-800" showInteractive={false} />
      </ReactFlow>
    </>
  );
}

export default function GraphView() {
  const sessions = useStore((s) => s.sessions);
  const events = useStore((s) => s.events);
  const selectedSessionId = useStore((s) => s.selectedSessionId);
  const focusInDetail = useStore((s) => s.focusInDetail);
  const theme = useStore((s) => s.theme);
  const [showDetail, setShowDetail] = useState(false);

  const edgeColor = theme === "dark" ? "#a78bfa" : "#7c3aed"; // violet-400 / violet-600
  const bgDotsColor = theme === "dark" ? "#27272a" : "#d4d4d8";

  const session: Session | undefined = useMemo(
    () => sessions.find((s) => s.id === selectedSessionId) ?? sessions[0],
    [sessions, selectedSessionId]
  );

  // The conversation is reconstructed from a single "winner" event; its id is
  // shared by every turn except possibly the appended response.
  const winnerEvent: LogEvent | null = useMemo(() => {
    const eid = session?.conversation[0]?.event_id;
    return events.find((e) => e.id === eid) ?? null;
  }, [events, session]);

  const turns = useMemo(
    () => (session ? expandTurns(session.conversation, winnerEvent) : []),
    [session, winnerEvent]
  );

  const defaultCollapsed = useMemo(() => {
    const set = new Set<number>();
    for (const t of turns) {
      if (t.kind === "tool_call" || t.kind === "tool_result" || t.kind === "system") {
        set.add(t.index);
      }
    }
    return set;
  }, [turns]);

  const [collapsedBySession, setCollapsedBySession] = useState<
    Record<string, Set<number>>
  >({});

  const collapsed = session
    ? collapsedBySession[session.id] ?? defaultCollapsed
    : new Set<number>();

  const toggle = useCallback(
    (idx: number) => {
      if (!session) return;
      setCollapsedBySession((prev) => {
        const cur = new Set(prev[session.id] ?? defaultCollapsed);
        if (cur.has(idx)) cur.delete(idx);
        else cur.add(idx);
        return { ...prev, [session.id]: cur };
      });
    },
    [session, defaultCollapsed]
  );

  const expandAll = () => {
    if (!session) return;
    setCollapsedBySession((prev) => ({ ...prev, [session.id]: new Set() }));
  };

  const collapseAll = () => {
    if (!session) return;
    setCollapsedBySession((prev) => ({
      ...prev,
      [session.id]: new Set(turns.map((t) => t.index)),
    }));
  };

  const jump = useCallback(
    (eventId: string, focusTarget: string) => {
      focusInDetail(eventId, focusTarget);
      setShowDetail(true);
    },
    [focusInDetail]
  );

  const { nodes, edges } = useMemo(
    () => buildGraph(turns, collapsed, toggle, jump, edgeColor),
    [turns, collapsed, toggle, jump, edgeColor]
  );

  if (!session) {
    return (
      <div className="flex items-center justify-center h-full text-zinc-500 text-sm">
        No session selected
      </div>
    );
  }

  return (
    <div className="flex h-full w-full">
      <div className="relative flex-1 min-w-0">
        <ReactFlowProvider>
          <GraphInner
            nodes={nodes}
            edges={edges}
            showDetail={showDetail}
            expandAll={expandAll}
            collapseAll={collapseAll}
            openDetail={() => setShowDetail(true)}
            turnsCount={turns.length}
            bgDotsColor={bgDotsColor}
          />
        </ReactFlowProvider>
      </div>
      {showDetail && (
        <div className="w-[42%] min-w-[420px] max-w-[720px] flex flex-col border-l border-zinc-800 bg-zinc-950">
          <div className="flex items-center justify-between px-2 py-1 border-b border-zinc-800 bg-zinc-900">
            <span className="text-[10px] uppercase tracking-wider text-zinc-400">
              Detail
            </span>
            <button
              onClick={() => setShowDetail(false)}
              className="p-1 text-zinc-500 hover:text-zinc-100"
              title="Close detail"
            >
              <X size={14} />
            </button>
          </div>
          <div className="flex-1 min-h-0">
            <Detail />
          </div>
        </div>
      )}
    </div>
  );
}
