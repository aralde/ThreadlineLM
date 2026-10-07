import Editor from "@monaco-editor/react";
import "../lib/monaco";

// Loaded lazily from Detail so Monaco stays out of the startup bundle.
export default function JsonViewer({ value, theme }: { value: unknown; theme: string }) {
  return (
    <Editor
      height="100%"
      theme={theme}
      language="json"
      value={JSON.stringify(value, null, 2)}
      options={{ readOnly: true, minimap: { enabled: false }, fontSize: 12 }}
    />
  );
}
