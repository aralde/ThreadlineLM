import { useEffect, useMemo, useRef, useState } from "react";
import Editor from "@monaco-editor/react";
import { useStore } from "../state/store";
import type { ChatMessage } from "../types";

function MessageBubble({
  m,
  bubbleId,
  registerRef,
  focusedTcId,
  focusedBubble,
}: {
  m: ChatMessage;
  bubbleId: string;
  registerRef: (key: string, el: HTMLDivElement | null) => void;
  focusedTcId: string | null;
  focusedBubble: boolean;
}) {
  const tone =
    m.role === "user"
      ? "border-sky-700/50 bg-sky-900/10"
      : m.role === "assistant"
      ? "border-emerald-700/50 bg-emerald-900/10"
      : m.role === "system"
      ? "border-zinc-700/50 bg-zinc-900"
      : "border-amber-700/50 bg-amber-900/10";

  const focusRing = focusedBubble
    ? "ring-2 ring-emerald-400 ring-offset-2 ring-offset-zinc-950"
    : "";

  return (
    <div
      ref={(el) => registerRef(bubbleId, el)}
      data-bubble-id={bubbleId}
      className={`border rounded p-2 transition-shadow duration-300 ${tone} ${focusRing}`}
    >
      <div className="text-[10px] uppercase tracking-wider text-zinc-500 mb-1">
        {m.role}
        {m.name ? ` · ${m.name}` : ""}
      </div>
      {m.content && (
        <div className="text-xs whitespace-pre-wrap text-zinc-200 font-mono">
          {m.content}
        </div>
      )}
      {m.tool_calls && m.tool_calls.length > 0 && (
        <div className="mt-2 space-y-1">
          {m.tool_calls.map((tc) => {
            const isFocused = focusedTcId === tc.id && tc.id.length > 0;
            return (
              <div
                key={tc.id}
                ref={(el) => tc.id && registerRef(`tc:${tc.id}`, el)}
                data-tc-id={tc.id}
                className={`text-[11px] font-mono bg-zinc-950 border rounded p-1.5 transition-shadow duration-300 ${
                  isFocused
                    ? "border-emerald-500 ring-2 ring-emerald-400 ring-offset-2 ring-offset-zinc-950"
                    : "border-zinc-800"
                }`}
              >
                <span className="text-violet-400">{tc.name}</span>
                <span className="text-zinc-500">(</span>
                <span className="text-zinc-300">{tc.arguments}</span>
                <span className="text-zinc-500">)</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function parseFocus(focusKey: string | null): { target: string; tcId: string | null } {
  if (!focusKey) return { target: "", tcId: null };
  const bare = focusKey.split("#")[0];
  if (bare.startsWith("tc:")) return { target: bare, tcId: bare.slice(3) };
  return { target: bare, tcId: null };
}

export default function Detail() {
  const events = useStore((s) => s.events);
  const id = useStore((s) => s.selectedEventId);
  const focusKey = useStore((s) => s.focusKey);
  const clearFocus = useStore((s) => s.clearFocus);
  const theme = useStore((s) => s.theme);
  const monacoTheme = theme === "dark" ? "vs-dark" : "vs";
  const [tab, setTab] = useState<"chat" | "request" | "response" | "raw">(
    "chat"
  );
  const refs = useRef<Map<string, HTMLDivElement>>(new Map());
  const registerRef = (key: string, el: HTMLDivElement | null) => {
    if (el) refs.current.set(key, el);
    else refs.current.delete(key);
  };

  const event = useMemo(() => events.find((e) => e.id === id) ?? null, [events, id]);

  const { target, tcId } = parseFocus(focusKey);

  // When a focus arrives, jump to chat tab and scroll the target into view.
  useEffect(() => {
    if (!focusKey || !event) return;
    setTab("chat");
    // Wait one paint so the chat list is mounted before scrolling.
    const handle = requestAnimationFrame(() => {
      const key = tcId ? `tc:${tcId}` : target;
      const el = refs.current.get(key);
      if (el) {
        el.scrollIntoView({ behavior: "smooth", block: "center" });
      }
    });
    // Clear the highlight after the animation so a repeat jump can re-trigger it.
    const timeout = window.setTimeout(() => clearFocus(), 1800);
    return () => {
      cancelAnimationFrame(handle);
      window.clearTimeout(timeout);
    };
  }, [focusKey, event, target, tcId, clearFocus]);

  if (!event) {
    return (
      <div className="flex items-center justify-center h-full text-zinc-500 text-sm">
        Select an event from the Timeline
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="px-3 py-2 border-b border-zinc-800 bg-zinc-900/60">
        <div className="flex items-center gap-3 text-xs">
          <span className="font-mono text-zinc-400">{event.id}</span>
          <span className="text-zinc-200">{event.provider}</span>
          <span className="text-zinc-400">/</span>
          <span className="text-zinc-200">{event.model}</span>
          <span className="ml-auto font-mono text-zinc-500">
            {event.duration_ms} ms · status {event.status}
          </span>
        </div>
        <div className="text-[10px] text-zinc-500 mt-1 font-mono">
          {event.upstream?.url ?? "—"}
        </div>
      </div>

      <div className="flex border-b border-zinc-800 bg-zinc-900/30 text-xs">
        {(["chat", "request", "response", "raw"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`px-3 py-1.5 capitalize ${
              tab === t
                ? "text-zinc-100 border-b border-emerald-400"
                : "text-zinc-500 hover:text-zinc-200"
            }`}
          >
            {t}
          </button>
        ))}
      </div>

      <div className="flex-1 min-h-0 overflow-auto">
        {tab === "chat" && (
          <div className="p-3 space-y-2">
            {event.request.messages.map((m, i) => {
              const bubbleId = `msg:${i}`;
              return (
                <MessageBubble
                  key={i}
                  m={m}
                  bubbleId={bubbleId}
                  registerRef={registerRef}
                  focusedTcId={tcId}
                  focusedBubble={!tcId && target === bubbleId}
                />
              );
            })}
            {event.response.assistant && (
              <MessageBubble
                m={event.response.assistant}
                bubbleId="resp"
                registerRef={registerRef}
                focusedTcId={tcId}
                focusedBubble={!tcId && target === "resp"}
              />
            )}
            {event.response.finish_reason && (
              <div className="text-[10px] text-zinc-500 italic">
                finish_reason: {event.response.finish_reason}
              </div>
            )}
          </div>
        )}
        {tab === "request" && (
          <Editor
            height="100%"
            theme={monacoTheme}
            language="json"
            value={JSON.stringify(event.request, null, 2)}
            options={{ readOnly: true, minimap: { enabled: false }, fontSize: 12 }}
          />
        )}
        {tab === "response" && (
          <Editor
            height="100%"
            theme={monacoTheme}
            language="json"
            value={JSON.stringify(event.response, null, 2)}
            options={{ readOnly: true, minimap: { enabled: false }, fontSize: 12 }}
          />
        )}
        {tab === "raw" && (
          <Editor
            height="100%"
            theme={monacoTheme}
            language="json"
            value={JSON.stringify(event.raw, null, 2)}
            options={{ readOnly: true, minimap: { enabled: false }, fontSize: 12 }}
          />
        )}
      </div>
    </div>
  );
}
