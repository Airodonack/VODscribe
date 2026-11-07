// The notes the app can be told to stop showing, each a setting that is on
// while it shows (crates/core `settings`): Settings has a switch for each in
// the pane of its subject, and Reset settings brings them all back. The note
// itself turns its switch off: its "Don't ask again", ticked (the permission
// note's, the Resolve note's).
// The VODscription offer after an add keeps the answer given instead
// (`foundVodscription`): Settings › Privacy has Ask, Use it and Don't use it.
// Looking VODscriptions up at all is the Privacy switch
// `ask_for_annotations`, a separate choice.

import type { Settings, UseVodscription } from "./api";

export type Ask = "ask_before_downloading" | "suggest_mov_for_resolve";

/** Every note that can be stopped. */
export const ASKS: readonly Ask[] = ["ask_before_downloading", "suggest_mov_for_resolve"];

/** Whether `ask` shows: on unless turned off, also before the settings are in. */
export const shows = (settings: Partial<Settings> | null | undefined, ask: Ask) => settings?.[ask] !== false;

/** What's done with a VODscription found for a VOD just added or refreshed:
 *  asked unless chosen, also before the settings are in. */
export const foundVodscription = (settings: Partial<Settings> | null | undefined): UseVodscription =>
  settings?.use_vodscription ?? "ask";

/** Where 0.7.0 and earlier kept each stopped, among the page's choices
 *  (stored.ts): "true" once its "Don't show this again" was ticked or it was
 *  answered. */
export const OLD_KEYS: Partial<Record<Ask, string>> = {
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
  const stopped = (Object.keys(OLD_KEYS) as Ask[]).filter((ask) => load(OLD_KEYS[ask]!) === "true");
  if (!stopped.length) return null;
  return {
    change: Object.fromEntries(stopped.map((ask) => [ask, false])),
    keys: stopped.map((ask) => OLD_KEYS[ask]!),
  };
}
