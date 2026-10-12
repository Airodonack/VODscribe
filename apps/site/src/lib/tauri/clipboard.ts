// Stands in for @tauri-apps/plugin-clipboard-manager.
export async function writeText(text: string) {
  await navigator.clipboard.writeText(text);
}
