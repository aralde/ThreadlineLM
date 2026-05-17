import { create } from "zustand";
import type { LogEvent, Session, SourceFile, Workspace } from "../types";

type View = "timeline" | "detail" | "graph" | "compare" | "metrics";
export type Theme = "dark" | "light";

const THEME_KEY = "threadlinelm:theme";

function loadTheme(): Theme {
  if (typeof window === "undefined") return "dark";
  const v = window.localStorage.getItem(THEME_KEY);
  return v === "light" ? "light" : "dark";
}

function applyTheme(t: Theme) {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.theme = t;
  document.documentElement.style.colorScheme = t;
}

interface AppState {
  files: SourceFile[];
  events: LogEvent[];
  sessions: Session[];
  warnings: string[];
  selectedEventId: string | null;
  selectedSessionId: string | null;
  view: View;
  search: string;
  compareIds: [string | null, string | null];
  /** Detail focus target: "msg:<i>" (request msg index), "resp" (response
   *  assistant), or "tc:<id>" (a specific tool_call). The token after the
   *  colon is suffixed with `#<nonce>` so repeated jumps to the same target
   *  still trigger the scroll/highlight effect. */
  focusKey: string | null;
  theme: Theme;
  toggleTheme: () => void;
  setWorkspace: (ws: Workspace, warnings: string[]) => void;
  mergeWorkspace: (ws: Workspace, warnings: string[]) => void;
  selectEvent: (id: string | null) => void;
  setSelectedEventId: (id: string | null) => void;
  selectSession: (id: string | null) => void;
  setView: (v: View) => void;
  setSearch: (s: string) => void;
  toggleCompare: (id: string) => void;
  focusInDetail: (eventId: string, target: string) => void;
  clearFocus: () => void;
  reset: () => void;
}

export const useStore = create<AppState>((set, get) => ({
  files: [],
  events: [],
  sessions: [],
  warnings: [],
  selectedEventId: null,
  selectedSessionId: null,
  view: "timeline",
  search: "",
  compareIds: [null, null],
  focusKey: null,
  theme: (() => {
    const t = loadTheme();
    applyTheme(t);
    return t;
  })(),
  toggleTheme: () => {
    const next: Theme = get().theme === "dark" ? "light" : "dark";
    if (typeof window !== "undefined") {
      window.localStorage.setItem(THEME_KEY, next);
    }
    applyTheme(next);
    set({ theme: next });
  },
  setWorkspace: (ws, warnings) =>
    set({
      files: ws.files,
      events: ws.events,
      sessions: ws.sessions,
      warnings,
      selectedEventId: ws.events[0]?.id ?? null,
      selectedSessionId: ws.sessions[0]?.id ?? null,
    }),
  mergeWorkspace: (ws, warnings) => {
    const cur = get();
    set({
      files: [...cur.files, ...ws.files],
      events: [...cur.events, ...ws.events],
      sessions: [...cur.sessions, ...ws.sessions],
      warnings: [...cur.warnings, ...warnings],
    });
  },
  selectEvent: (id) => set({ selectedEventId: id, view: "detail" }),
  setSelectedEventId: (id) => set({ selectedEventId: id }),
  selectSession: (id) => set({ selectedSessionId: id }),
  setView: (view) => set({ view }),
  setSearch: (search) => set({ search }),
  focusInDetail: (eventId, target) => {
    // Append a nonce so repeated jumps to the same target re-trigger the effect.
    const key = `${target}#${Date.now().toString(36)}`;
    set({ selectedEventId: eventId, focusKey: key });
  },
  clearFocus: () => set({ focusKey: null }),
  toggleCompare: (id) => {
    const [a, b] = get().compareIds;
    if (a === id) set({ compareIds: [b, null] });
    else if (b === id) set({ compareIds: [a, null] });
    else if (!a) set({ compareIds: [id, b] });
    else if (!b) set({ compareIds: [a, id] });
    else set({ compareIds: [id, a] });
  },
  reset: () =>
    set({
      files: [],
      events: [],
      sessions: [],
      warnings: [],
      selectedEventId: null,
      selectedSessionId: null,
      compareIds: [null, null],
    }),
}));
