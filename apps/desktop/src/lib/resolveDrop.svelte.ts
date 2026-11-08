// The note at a drop on DaVinci Resolve on Linux, which can't open MP4 clips
// (no H.264 in its free edition, no AAC in any): they come in blank. Asked
// once the drag is over, where the drop was, never before: the app refuses
// Resolve the MP4 where X11 names it ("refused"), or can't tell where it was
// let go (Wayland) while Resolve is installed ("maybe"; clips.rs `DragEnded`,
// src-tauri resolve.rs). The same for an edit's video dragged out. "Switch to
// MOV" makes clips and edits as MOV DNxHR from then on, and the clips kept
// from the drop, or the edit, are made again so; "Don't ask again",
// ticked, stops it (the setting `suggest_mov_for_resolve`, asks.ts), and
// Settings › Encoding turns it back on. The ffmpeg in use is asked as the
// note shows: one without DNxHR isn't offered the switch, and the note says
// why and what MOV it can make, if any; an answer given before it said so
// switches nothing and says the same.

import { api, type FfmpegCodecs } from "./api";
import { settings } from "./appSettings.svelte";
import { toast } from "./toasts.svelte";

export type ResolveDrop = "refused" | "maybe";

/** Why Switch to MOV isn't offered, with VODscribe's own ffmpeg in use: no DNxHR, but ProRes. */
export const PICK_PRORES_OWN = "VODscribe's ffmpeg can't make MOV DNxHR, so pick MOV ProRes in Settings › Encoding.";
/** The same, with the system's ffmpeg in use. */
export const PICK_PRORES_SYSTEM =
  "Your system's ffmpeg can't make MOV DNxHR, so pick MOV ProRes in Settings › Encoding.";
/** Why Switch to MOV isn't offered, with VODscribe's own ffmpeg in use: neither DNxHR nor ProRes. */
export const NO_MOV_OWN = "VODscribe's ffmpeg can't make MOV clips.";
/** The same, with the system's ffmpeg in use. */
export const NO_MOV_SYSTEM = "Your system's ffmpeg can't make MOV clips.";

/** Why Switch to MOV isn't offered, given what the ffmpeg in use can't make; null when it can make DNxHR. */
export function cantSwitch({ missing, own }: FfmpegCodecs): string | null {
  if (!missing.includes("dnxhr")) return null;
  if (!missing.includes("prores")) return own ? PICK_PRORES_OWN : PICK_PRORES_SYSTEM;
  return own ? NO_MOV_OWN : NO_MOV_SYSTEM;
}

/** Under "Don't ask again", once ticked. */
export const WHERE_UNDONE = "You can turn it back on in Settings › Encoding.";

/** What was dropped on Resolve: clips, or an edit's video. */
export type Dropped = "clip" | "edit";

/** What the note says of a drop of `count` clips (or an edit) on Resolve, `how` it went. */
export function noteText(how: ResolveDrop, count: number, what: Dropped = "clip"): string {
  const dropped = what === "edit" ? "the edit wasn't" : count > 1 ? "the clips weren't" : "the clip wasn't";
  return how === "refused"
    ? `DaVinci Resolve can't open MP4 clips on Linux, so ${dropped} dropped there. MOV clips work.`
    : "Dropping in DaVinci Resolve? It can't open MP4 clips on Linux. MOV clips work.";
}

/** What's said once clips are switched to MOV, after a drop `how` it went,
 *  of `count` clips: kept in the clip bar if refused, to drag again; or of
 *  an edit, rendered again as MOV if refused. */
export function switched(how: ResolveDrop, count: number, what: Dropped = "clip"): string {
  // An edit's video is rendered again either way (dragout.ts `dragEditOut`).
  if (what === "edit") return "Clips are MOV now. Drag the edit again once it's rendered.";
  const them = count > 1 ? "them" : "it";
  return how === "refused"
    ? `Clips are MOV now. Drag ${them} again from the clip bar.`
    : `Clips are MOV now. Drag ${them} again.`;
}

interface Shown {
  how: ResolveDrop;
  count: number;
  what: Dropped;
  /** Makes the clips kept from the drop (or the edit) again, as MOV. */
  remake: () => void;
  /** Why Switch to MOV isn't offered (`cantSwitch`), or null if it is (the ffmpeg can't say, too). */
  cant: Promise<string | null>;
}

class ResolveNote {
  shown = $state<Shown | null>(null);
  /** Why Switch to MOV isn't offered, once the ffmpeg in use says it lacks DNxHR (said after the note's text); else null. */
  cantSwitch = $state<string | null>(null);
  #asked = 0;

  /** A drag of `count` clips (or an edit) was let go on Resolve, or perhaps (`how`). */
  show(how: ResolveDrop, count: number, remake: () => void = () => {}, what: Dropped = "clip") {
    const n = ++this.#asked;
    const cant = api.missingClipCodecs().then(cantSwitch, () => null);
    this.shown = { how, count, what, remake, cant };
    this.cantSwitch = null;
    void cant.then((why) => {
      if (n === this.#asked) this.cantSwitch = why;
    });
  }

  /** Answered: clips as MOV from now on (`mov`), or not; and not asked again (`hide`). */
  async answer(mov: boolean, hide: boolean) {
    const shown = this.shown;
    this.shown = null;
    if (!shown || (!mov && !hide)) return;
    const cant = mov ? await shown.cant : null;
    if (cant) toast.showUnlessUp(cant);
    const switching = mov && !cant;
    if (!switching && !hide) return;
    await settings.update({
      ...(hide ? { suggest_mov_for_resolve: false } : {}),
      ...(switching ? { clip_codec: "dnxhr" as const, clip_format: "mov" as const } : {}),
    });
    if (!switching) return;
    shown.remake();
    toast.showUnlessUp(switched(shown.how, shown.count, shown.what));
  }
}

export const resolveNote = new ResolveNote();
