// Stands in for @tauri-apps/plugin-dialog; the controls that use it are off in the demo.
export async function open(_options: unknown): Promise<null> {
  return null;
}

/** The browser's own question: the demo's controls that ask are off anyway. */
export async function confirm(message: string, _options?: unknown): Promise<boolean> {
  return window.confirm(message);
}
