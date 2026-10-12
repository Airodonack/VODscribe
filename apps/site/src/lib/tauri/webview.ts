// Stands in for @tauri-apps/api/webview: a web page gets no file drops.
export function getCurrentWebview() {
  return { onDragDropEvent: async (_handler: unknown) => () => {} };
}
