// The one-time hint for DaVinci Resolve on Linux, which can't read MP4 clips
// (no H.264 in its free edition, no AAC in any): they come in blank. Shown
// after the first clip is dragged out (dragout.ts), only where Resolve is
// installed and clips are still MP4, in the library (FormatHint.svelte).

import { api } from "./api";
import { settings } from "./appSettings.svelte";
import { os } from "./platform";
import { load, save } from "./stored";

const SEEN = "vodscribe.resolveHintSeen";

class FormatHint {
  shown = $state(false);

  /** A clip was dragged out: the hint, if it applies and hasn't been seen. */
  async dragged() {
    if (os !== "linux" || load(SEEN) === "true") return;
    const [, resolve] = await Promise.all([settings.load(), api.resolveInstalled().catch(() => false)]);
    if (resolve && (settings.value?.clip_format ?? "mp4") === "mp4") this.shown = true;
  }

  /** Answered: clips as MOV from now on (`useMov`), or not; either way, not shown again. */
  answer(useMov: boolean) {
    this.shown = false;
    save(SEEN, "true");
    if (useMov) settings.update({ clip_format: "mov" });
  }
}

export const formatHint = new FormatHint();
