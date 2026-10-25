// Small things remembered on this computer: a choice, a setting of the
// player's. The app keeps them in its data folder (crates/core `choices`),
// not in the web view's storage, which is per origin, and an installed app's
// page comes from a new port each launch (app_server.rs). They're read once,
// before the page starts (`start`: hooks.client.ts; the website's Demo.svelte
// before it loads the app), so `load` can answer at once; `save` writes each
// through. The website's stand-in keeps them in the browser's storage instead
// (apps/site/src/lib/tauri/core.ts).

import { invoke } from "@tauri-apps/api/core";

let kept = new Map<string, string>();

/** Reads every choice kept, once, before anything asks for one. If they
 *  can't be read, nothing is remembered this time, which is fine. */
export async function start() {
  kept = new Map(Object.entries(await invoke<Record<string, string>>("kept_choices").catch(() => ({}))));
  // A dev build's page has kept the same origin all along, so what it
  // remembered is still in the web view's storage: brought over the first
  // time. (An installed app's is empty, a new origin each launch.)
  if (kept.size) return;
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      const value = key?.startsWith("vodscribe.") ? localStorage.getItem(key) : null;
      if (key && value !== null) save(key, value);
    }
  } catch {
    // No storage here: nothing to bring over.
  }
}

/** The value kept under `key`, or null. */
export function load(key: string): string | null {
  return kept.get(key) ?? null;
}

// Sent one at a time, in order, so the last value given is the one kept;
// one changed again while waiting (the volume dragged) is sent once, as it is then.
const unsent = new Map<string, string>();
let sending = false;

async function send() {
  if (sending) return;
  sending = true;
  try {
    for (const [key, value] of unsent) {
      unsent.delete(key);
      await invoke("keep_choice", { key, value }).catch(() => {
        // Not remembered this time.
      });
    }
  } finally {
    sending = false;
  }
}

/** Keeps `value` under `key`, if it can. */
export function save(key: string, value: string) {
  if (kept.get(key) === value) return;
  kept.set(key, value);
  unsent.delete(key);
  unsent.set(key, value);
  void send();
}
