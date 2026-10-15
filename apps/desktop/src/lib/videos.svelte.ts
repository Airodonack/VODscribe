// Whole videos of YouTube, Twitch and Kick VODs downloaded on request, to play
// (and clip) here (src-tauri videos.rs): which are downloading, how far, and
// which failed. Their rows show it (VodRow.svelte).

import { SvelteMap } from "svelte/reactivity";
import { api, events } from "./api";
import { DEMO } from "./demo";

class VideoDownloads {
  /** How far each download is, 0..1 (0 while waiting its turn). */
  progress = new SvelteMap<number, number>();
  /** Why a download failed, by VOD. */
  errors = new SvelteMap<number, string>();
  #started = false;

  /** Follows the downloads; the first caller starts it. */
  start() {
    if (this.#started || DEMO) return;
    this.#started = true;
    events.videoProgress(({ id, progress, error }) => {
      if (progress === null) this.progress.delete(id);
      else this.progress.set(id, progress);
      if (error) this.errors.set(id, error);
    });
    api.videoDownloads().then(
      (ids) => ids.forEach((id) => this.progress.has(id) || this.progress.set(id, 0)),
      (e) => console.error("video downloads", e),
    );
  }

  async keep(id: number) {
    this.errors.delete(id);
    this.progress.set(id, 0);
    try {
      await api.keepVideo(id);
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
