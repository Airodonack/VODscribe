// Asking before something that can't be undone. The dialog plugin replaces
// window.confirm with an async version whose Promise `if` reads as yes, so
// asking that way never stopped anything: ask with the plugin's dialog, and
// await the answer. A dialog that fails to show counts as no.

import { confirm, message } from "@tauri-apps/plugin-dialog";

/** Asks `message`, with `ok` and `cancel` as the buttons; true if `ok` was chosen. */
export function ask(message: string, ok: string, cancel = "Cancel"): Promise<boolean> {
  return confirm(message, { kind: "warning", okLabel: ok, cancelLabel: cancel }).catch((e) => {
    console.error("couldn't ask", e);
    return false;
  });
}

/** Asks `text` with three buttons, `yes`, `no` and `cancel`: the label of `yes` or `no` if one was
 *  chosen, else null (`cancel`, Escape, or a dialog that fails to show). */
export async function choose(text: string, yes: string, no: string, cancel: string): Promise<string | null> {
  try {
    const answer = await message(text, { kind: "warning", buttons: { yes, no, cancel } });
    return answer === yes || answer === no ? answer : null;
  } catch (e) {
    console.error("couldn't ask", e);
    return null;
  }
}
