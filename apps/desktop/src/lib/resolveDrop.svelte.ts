// The note at a drop on DaVinci Resolve on Linux, which can't open MP4 clips
// (no H.264 in its free edition, no AAC in any): they come in blank. Asked
// once the drag is over, where the drop was, never before: the app refuses
// Resolve the MP4 where X11 names it ("refused"), or can't tell where it was
// let go (Wayland) while Resolve is installed ("maybe"; clips.rs `DragEnded`,
// src-tauri resolve.rs). The same for an edit's video dragged out. "Switch to
// MOV" makes clips and edits as MOV DNxHR from then on, and the clips kept
// from the drop, or the edit, are made again so; "Don't show again",
// ticked, stops it (the setting `suggest_mov_for_resolve`, asks.ts), and
// Settings › Encoding turns it back on.

import { settings } from "./appSettings.svelte";
import { toast } from "./toasts.svelte";

export type ResolveDrop = "refused" | "maybe";

/** Under "Don't show again", once ticked. */
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
}

class ResolveNote {
  shown = $state<Shown | null>(null);

  /** A drag of `count` clips (or an edit) was let go on Resolve, or perhaps (`how`). */
  show(how: ResolveDrop, count: number, remake: () => void = () => {}, what: Dropped = "clip") {
    this.shown = { how, count, what, remake };
  }

  /** Answered: clips as MOV from now on (`mov`), or not; and not asked again (`hide`). */
  async answer(mov: boolean, hide: boolean) {
    const shown = this.shown;
    this.shown = null;
    if (!shown || (!mov && !hide)) return;
    await settings.update({
      ...(hide ? { suggest_mov_for_resolve: false } : {}),
      ...(mov ? { clip_codec: "dnxhr" as const, clip_format: "mov" as const } : {}),
    });
    if (!mov) return;
    shown.remake();
    toast.showUnlessUp(switched(shown.how, shown.count, shown.what));
  }
}

export const resolveNote = new ResolveNote();
