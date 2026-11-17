import { invoke } from "@tauri-apps/api/core";
import type { LoadResult } from "./types";

export const ipc = {
  loadFile: (path: string, merge = false) =>
    invoke<LoadResult>("load_file", { path, merge }),
  loadText: (name: string, content: string, merge = true) =>
    invoke<LoadResult>("load_text", { name, content, merge }),
  exportSessionMarkdown: (sessionId: string) =>
    invoke<string>("export_session_markdown", { sessionId }),
  exportWorkspaceJson: () => invoke<string>("export_workspace_json"),
  startWatch: (path: string) => invoke<LoadResult>("start_watch", { path }),
  stopWatch: () => invoke<void>("stop_watch"),
};
