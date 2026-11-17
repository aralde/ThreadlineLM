import type { LogEvent } from "../types";

export const isTranscript = (e: LogEvent) => e.endpoint === "claude-code/transcript";
