// Removing a VOD from the library: its row's Remove (VodRow.svelte), or
// several selected at once (bulk.ts), the same way. What's asked first is
// the caller's; this is what happens after.

import { SvelteSet } from "svelte/reactivity";
import { api, type Vod } from "./api";
import { choose } from "./ask";
import { edits } from "./edits.svelte";
import { listed } from "./platform";

/** The VODs being removed: their rows say so meanwhile (a long transcript
 *  takes a few seconds to erase), until the news that each is gone
 *  (App.svelte), or it fails. */
export const removing = new SvelteSet<number>();

/** The two ways to remove a VOD that edits use. */
export const REMOVE = "Remove from the library";
export const DELETE = "Remove and delete the video";

/** A sentence about the edits with VOD `vodId`'s clips, between `before` and `after`; empty if none. */
export function inEdits(vodId: number, before: string, after: string): string {
  const names = edits.forVod(vodId).map((e) => `“${e.title}”`);
  if (!names.length) return "";
  return `${before}${names.length === 1 ? "the edit" : "the edits"} ${listed(names)}${after}`;
}

/**
 * Removes `vod` (asked already), its video too if `deleteVideo`: then the
 * stretches edits use are kept as videos of their own first ("realized"),
 * and if that fails (offline, a full disk) it asks again what to do. An
 * error is given to `onerror`.
 */
export async function removeVod(vod: Vod, deleteVideo: boolean, onerror: (message: string) => void): Promise<void> {
  removing.add(vod.id);
  let anyway = false;
  for (;;) {
    try {
      await api.deleteVod(vod.id, deleteVideo, anyway);
      return;
    } catch (e) {
      const text = String(e);
      if (!text.startsWith("realize: ")) {
        removing.delete(vod.id);
        onerror(text);
        return;
      }
      const answer = await choose(
        `The stretches of “${vod.title}”${inEdits(vod.id, " in ", "")} couldn't be kept: ${text.slice("realize: ".length)} Deleting the video anyway leaves gaps in them.`,
        "Try again",
        "Delete anyway",
        "Keep the video",
      );
      if (answer === "Delete anyway") anyway = true;
      // Kept: removed from the library only.
      else if (answer !== "Try again") deleteVideo = false;
    }
  }
}
