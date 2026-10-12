// Stands in for @tauri-apps/api/app. The demo shows which app version it's built from.
export async function getVersion(): Promise<string> {
  return `${__APP_VERSION__}_demo`;
}
