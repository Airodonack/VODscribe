// Asking before something that can't be undone. The dialog plugin replaces
// window.confirm with an async version whose Promise `if` reads as yes, so
// asking that way never stopped anything: ask with the plugin's dialog, and
// await the answer. A dialog that fails to show counts as no.

import { confirm } from "@tauri-apps/plugin-dialog";

/** Asks `message`, with `ok` and `cancel` as the buttons; true if `ok` was chosen. */
export function ask(message: string, ok: string, cancel = "Cancel"): Promise<boolean> {
  return confirm(message, { kind: "warning", okLabel: ok, cancelLabel: cancel }).catch((e) => {
    console.error("couldn't ask", e);
    return false;
  });
}
