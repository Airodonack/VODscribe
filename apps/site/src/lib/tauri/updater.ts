// Stands in for @tauri-apps/plugin-updater: the demo is always current.
export type Update = { version: string; downloadAndInstall(onEvent: unknown): Promise<void> };
export async function check(): Promise<Update | null> {
  return null;
}
