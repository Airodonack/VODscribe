// A site's VOD's "Refresh details", from its row's menu (Library.svelte):
// its site is asked about it again for what the library lacks of a VOD added
// before it kept it, and nothing is deleted, as removing and adding it again
// would delete its transcript, chat, marks and clips. For now that's the
// channel's login and page (vods.rs `refresh_details`), so its streamer card
// links to the channel; later it may also refresh the video's other details
// from the site, such as its description, which VODscribe doesn't show yet.
// The file host is asked again too, unless Settings › Privacy turned that
// off: for its VODscription, offered as after an add (the app's
// `annotations-found`), and for the channel's streamer card, past the
// session's cache (streamers.ts `CardCache.fresh`), so one published since
// shows. It runs beside a download or transcription of the VOD; a second
// press while one runs does nothing, and a failure says so and changes nothing.
// Asking the site takes seconds (yt-dlp), so the row says "Refreshing
// details…" meanwhile (VodRow.svelte), as it does while a VOD is removed, and
// what's said after names the VOD. The file host's answers aren't waited
// for: an offer, or the card, comes when it's in.

import { SvelteSet } from "svelte/reactivity";
import { api, type Vod } from "./api";
import { cards, channelCard } from "./streamers.svelte";
import { toast } from "./toasts.svelte";

/** The VODs whose details are being refreshed: their rows say so. */
export const refreshing = new SvelteSet<number>();

/** Refreshes `vod`'s details, and says how that went. */
export async function refreshDetails(vod: Vod) {
  if (refreshing.has(vod.id)) return;
  refreshing.add(vod.id);
  try {
    const fresh = await api.refreshDetails(vod.id);
    // One for it was under way already, which says how it went.
    if (!fresh) return;
    const { member, account } = channelCard(fresh);
    void cards.fresh(member, account);
    toast.show(`Refreshed the details of “${fresh.title}”.`);
  } catch (e) {
    toast.error(`Couldn't refresh the details of “${vod.title}”. ${e}`);
  } finally {
    refreshing.delete(vod.id);
  }
}
