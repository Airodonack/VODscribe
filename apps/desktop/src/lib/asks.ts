// The notes the app can be told to stop showing, each a setting that is on
// while it shows (crates/core `settings`): Settings has a switch for each in
// the pane of its subject, and Reset settings brings them all back. The note
// itself turns its switch off: the permission note's "Don't show this
// again", ticked; so the Resolve note's "Don't show again". The
// VODscription offer's "No, and don't show again" is the Privacy switch
// `ask_for_annotations`.

import type { Settings } from "./api";

export type Ask = "ask_before_downloading" | "suggest_mov_for_resolve";

/** Whether `ask` shows: on unless turned off, also before the settings are in. */
export const shows = (settings: Partial<Settings> | null | undefined, ask: Ask) => settings?.[ask] !== false;

/** Where 0.7.0 and earlier kept each stopped, among the page's choices
 *  (stored.ts): "true" once its "Don't show this again" was ticked or it was answered. */
export const OLD_KEYS: Record<Ask, string> = {
  ask_before_downloading: "vodscribe.clipNoteHidden",
  suggest_mov_for_resolve: "vodscribe.resolveHintSeen",
};

/** What an old key holds once brought into the settings, so it's read once. */
export const BROUGHT_OVER = "in settings";

/** The notes stopped under an old key, as the change to the settings that
 *  keeps them stopped, and those keys; null when there are none. */
export function broughtOver(
  load: (key: string) => string | null,
): { change: Partial<Settings>; keys: string[] } | null {
  const stopped = (Object.keys(OLD_KEYS) as Ask[]).filter((ask) => load(OLD_KEYS[ask]) === "true");
  if (!stopped.length) return null;
  return {
    change: Object.fromEntries(stopped.map((ask) => [ask, false])),
    keys: stopped.map((ask) => OLD_KEYS[ask]),
  };
}
