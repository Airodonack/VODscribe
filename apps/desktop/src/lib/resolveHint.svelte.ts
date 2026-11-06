// The one-time hint for DaVinci Resolve on Linux, which can't read MP4 clips
// (no H.264 in its free edition, no AAC in any): they come in blank. Shown
// after the first clip is dragged out (dragout.ts), only where Resolve is
// installed and clips are still MP4, in the library (FormatHint.svelte).
// Answered, it's off; Settings › Encoding turns it back on (the setting
// `suggest_mov_for_resolve`, asks.ts).

import { api } from "./api";
import { settings } from "./appSettings.svelte";
import { shows } from "./asks";
import { os } from "./platform";

class FormatHint {
  shown = $state(false);

  /** A clip was dragged out: the hint, if it applies and hasn't been seen. */
  async dragged() {
    if (os !== "linux") return;
    await settings.load();
    if (!this.#applies()) return;
    // Settings may have changed while asking.
    if ((await api.resolveInstalled().catch(() => false)) && this.#applies()) this.shown = true;
  }

  /** Turned on, and clips still MP4. */
  #applies() {
    return shows(settings.value, "suggest_mov_for_resolve") && (settings.value?.clip_format ?? "mp4") === "mp4";
  }

  /** Answered: clips as MOV from now on (`useMov`), DNxHR LB as the hint
   *  says, or not; either way, not shown again. */
  answer(useMov: boolean) {
    this.shown = false;
    settings.update({
      suggest_mov_for_resolve: false,
      ...(useMov ? { clip_format: "mov" as const, clip_codec: "dnxhr" as const, clip_quality: "proxy" as const } : {}),
    });
  }
}

export const formatHint = new FormatHint();
