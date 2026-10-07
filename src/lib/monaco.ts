// Bundle Monaco locally instead of letting @monaco-editor/react fetch it from
// a CDN at runtime: the app must work offline and under a strict CSP.
import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";
import "monaco-editor/esm/vs/language/json/monaco.contribution.js";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker.js?worker";
import JsonWorker from "monaco-editor/esm/vs/language/json/json.worker.js?worker";

self.MonacoEnvironment = {
  getWorker: (_workerId, label) =>
    label === "json" ? new JsonWorker() : new EditorWorker(),
};

loader.config({ monaco });
