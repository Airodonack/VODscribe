// Whole videos of YouTube, Twitch and Kick VODs downloaded on request, to play
// (and clip) here (src-tauri videos.rs): which are downloading, how far, and
// which failed. Their rows show it (VodRow.svelte).

import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { api, events } from "./api";
import { DEMO } from "./demo";

class VideoDownloads {
  /** How far each download is, 0..1 (0 while waiting its turn). */
  progress = new SvelteMap<number, number>();
  /** Why a download failed, by VOD. */
  errors = new SvelteMap<number, string>();
  /** The VODs whose download plays only once it's all in: not streamed as
   *  it comes, so the timeline can't say how far it is. */
  playsOnceIn = new SvelteSet<number>();
  #started = false;

  /** Follows the downloads; the first caller starts it. */
  start() {
    if (this.#started || DEMO) return;
    this.#started = true;
    // Those heard of before the list answers: their news is newer than it (on
    // Linux an event from another thread can overtake an answer, page.rs), so
    // one that ended meanwhile isn't shown waiting again.
    const heard = new Set<number>();
    events.videoProgress(({ id, progress, error, streamed }) => {
      heard.add(id);
      if (progress === null) this.progress.delete(id);
      else this.progress.set(id, progress);
      if (progress === null || streamed) this.playsOnceIn.delete(id);
      else this.playsOnceIn.add(id);
      if (error) this.errors.set(id, error);
    });
    api.videoDownloads().then(
      (ids) => ids.forEach((id) => heard.has(id) || this.progress.has(id) || this.progress.set(id, 0)),
      (e) => console.error("video downloads", e),
    );
  }

  /** Downloads VOD `id`'s video, at most `height` tall (else the Storage setting). */
  async keep(id: number, height?: number) {
    this.errors.delete(id);
    this.progress.set(id, 0);
    try {
      await api.keepVideo(id, height);
    } catch (e) {
      this.progress.delete(id);
      this.errors.set(id, String(e));
    }
  }

  stop(id: number) {
    api.stopVideo(id).catch((e) => this.errors.set(id, String(e)));
  }
}

export const videoDownloads = new VideoDownloads();
