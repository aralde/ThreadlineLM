# ThreadlineLM

> Inspect, replay and graph LLM agent traces offline — operatorlm, OpenTelemetry GenAI and LiteLLM audit logs.

Offline forensic viewer for LLM calls. Drop a `.log` / `.jsonl` file on the window and get a timeline, per-call detail, per-session conversation reconstruction, flow graph (including tool calls), side-by-side comparison and metrics.

- **Stack**: Tauri 2 + React + TypeScript + Vite + Tailwind v4. Rust backend.
- **Privacy**: 100% local, in-memory. Zero network, zero telemetry.
- **Canonical format** (MVP): OpenAI-compatible audit JSONL. See `inputExample/audit.log`.

Status: early (0.1.0).

## Requirements

- Node 20+
- pnpm 10+
- Stable Rust toolchain (`rustup`)
- Windows: Microsoft Edge WebView2 (usually preinstalled on Windows 10/11)
- Linux: Tauri runtime dependencies (webkit2gtk, libayatana-appindicator, etc.)

## Development

```bash
pnpm install
pnpm tauri dev
```

Drop `inputExample/audit.log` onto the window. You should see:

- 9 rows in Timeline with providers `gemini`, `groq`, etc.
- Detail with request/response and the reconstructed chat.
- Graph for the selected session (user / assistant / tool_call nodes).
- Metrics with p50/p95 latency, tokens and estimated cost.

Shortcuts: `Ctrl+1..5` to switch tabs.

## Tests

```bash
cd src-tauri
cargo test
```

The `parses_input_example` test validates the parser against `inputExample/audit.log`.

## Build

```bash
pnpm tauri build
```

Produces per-platform binaries under `src-tauri/target/release/bundle/` (Windows MSI/NSIS, macOS DMG, Linux AppImage/deb).

### Portable binary (Windows)

For a single portable `.exe` (no installer, copy-paste to a USB stick):

```powershell
pnpm tauri build --no-bundle
```

Output: `src-tauri/target/release/llm-visor.exe` (~10–15 MB).

Other combinations:

| Command | Output |
|---|---|
| `pnpm tauri build` | `.exe` + NSIS installer + `.msi` |
| `pnpm tauri build --no-bundle` | portable `.exe` only |
| `pnpm tauri build --bundles nsis` | NSIS installer only |

**WebView2**: the `.exe` depends on Microsoft Edge WebView2 as its rendering engine. It is preinstalled on Windows 11. On older Windows 10 builds, if missing, it is downloaded on first launch.

For a *truly portable* build that does not depend on the internet or the system WebView2 (≈ +150 MB overhead), embed the "Fixed Version Runtime" — download it from [aka.ms/webview2](https://developer.microsoft.com/microsoft-edge/webview2/) and add to `src-tauri/tauri.conf.json`:

```json
"bundle": {
  "windows": {
    "webviewInstallMode": {
      "type": "fixedRuntime",
      "path": "path/to/Microsoft.WebView2.FixedVersionRuntime.x.y.z.x64"
    }
  }
}
```

In that case ship the **entire folder** (not just the `.exe`), because WebView2 sits next to it.

### Cross-compiling

Cross-building between Windows / macOS / Linux from a single host is non-trivial — each target needs its native toolchain. For multi-OS releases the simplest path is a GitHub Actions matrix with `windows-latest`, `macos-latest`, `ubuntu-latest` runners.

## Project layout

```
src/                       React + TS frontend
  components/              UI per view (Timeline, Detail, Graph, Compare, Metrics)
  state/store.ts           Zustand store
  ipc.ts                   typed wrappers around invoke()
src-tauri/
  src/
    parser/                detect, openai_audit, raw_text
    session.rs             grouping by client + 10-min window
    export.rs              session_to_markdown
    commands.rs            #[tauri::command] exposed to the frontend
inputExample/audit.log     canonical fixture (9 events)
```

## Contributing

Issues and PRs welcome at <https://github.com/adeibe/ThreadlineLM>.

## License

MIT — see [LICENSE](LICENSE).
