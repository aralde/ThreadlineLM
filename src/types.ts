// Tipos compartidos. En el futuro se generarán con ts-rs desde Rust;
// por ahora los mantenemos a mano sincronizados con src-tauri/src/model.rs.

export type Provider =
  | "openai"
  | "anthropic"
  | "gemini"
  | "groq"
  | "mistral"
  | "cohere"
  | "ollama"
  | "deepseek"
  | "xai"
  | "azure"
  | "unknown";

export interface Upstream {
  url: string;
  status: number | null;
}

export interface ChatMessage {
  role: string; // "system" | "user" | "assistant" | "tool"
  content: string | null;
  name?: string | null;
  tool_call_id?: string | null;
  tool_calls?: ToolCall[];
  /** Provider chain-of-thought / reasoning summary, when exposed. */
  reasoning?: string | null;
}

export interface ToolCall {
  id: string;
  name: string;
  arguments: string; // JSON string
}

export interface Usage {
  prompt_tokens: number | null;
  completion_tokens: number | null;
  total_tokens: number | null;
}

export interface RequestPayload {
  model: string;
  messages: ChatMessage[];
  stream: boolean;
  tools_declared: number; // count of tools declared in the request
}

export interface ResponsePayload {
  assistant: ChatMessage | null;
  finish_reason: string | null;
  usage: Usage;
}

export interface LogEvent {
  id: string;
  ts: string; // ISO-8601
  client: string;
  ua: string | null;
  provider: Provider;
  model: string;
  endpoint: string;
  request: RequestPayload;
  response: ResponsePayload;
  upstream: Upstream | null;
  status: number;
  duration_ms: number;
  raw: unknown;
}

export interface SourceFile {
  path: string;
  name: string;
  bytes: number;
  events: number;
}

export interface ConversationTurn {
  event_id: string;
  role: string;
  content: string;
  tool_calls?: ToolCall[];
  reasoning?: string | null;
}

export interface Session {
  id: string;
  client: string;
  started_at: string;
  ended_at: string;
  event_ids: string[];
  conversation: ConversationTurn[];
}

export interface Workspace {
  files: SourceFile[];
  events: LogEvent[];
  sessions: Session[];
}

export interface LoadResult {
  workspace: Workspace;
  warnings: string[];
}
