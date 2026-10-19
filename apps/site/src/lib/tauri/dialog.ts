// Stands in for @tauri-apps/plugin-dialog; the controls that use it are off in the demo.
export async function open(_options: unknown): Promise<null> {
  return null;
}

/** The browser's own question: the demo's controls that ask are off anyway. */
export async function confirm(message: string, _options?: unknown): Promise<boolean> {
  return window.confirm(message);
}

/** A question with buttons of its own: in the demo, the browser's, Yes for OK and the third for Cancel. */
export async function message(
  text: string,
  options?: { buttons?: { yes: string; no: string; cancel: string } },
): Promise<string> {
  const buttons = options?.buttons;
  if (!buttons) return window.confirm(text) ? "Ok" : "Cancel";
  return window.confirm(`${text}\n\nOK: ${buttons.yes}. Cancel: ${buttons.cancel}.`) ? buttons.yes : buttons.cancel;
}
