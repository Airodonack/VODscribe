// Dragging a clip out of the window, into a video editor or a folder. A chip
// starts the system's drag once the pointer moves; a press held on the clip
// bar or the picture first flies the clip's chip to the pointer, which shows
// what's being picked up without a word of explanation.

import { api, type Clip } from "./api";
import { formatTime } from "./platform";

/** How long a press is held before it picks the clip up. */
export const HOLD_MS = 250;
const FLY_MS = 150;

/** The file of the clip being dragged out, so the library doesn't add it if
 *  it's dropped back on the window (Library.svelte). */
let outgoing: string | null = null;
let forget: ReturnType<typeof setTimeout> | undefined;

/** Whether `paths` are just the clip being dragged out. */
export function isOutgoing(paths: string[]): boolean {
  return outgoing !== null && paths.length > 0 && paths.every((p) => p === outgoing);
}

/** Starts the system drag of `clip`, if its file is made. */
export async function dragOut(clip: Clip): Promise<void> {
  if (clip.status !== "ready" || !clip.file_path) return;
  outgoing = clip.file_path;
  clearTimeout(forget);
  forget = setTimeout(() => (outgoing = null), 60_000);
  await api.dragClip(clip.id).catch((e) => console.error("drag", e));
}

/**
 * Flies a chip for `clip` from `from` (where it's shown on the clip bar or in
 * the strip) to the pointer at (`x`, `y`), then starts the system drag, if the
 * button is still down by then (`held`).
 */
export async function flyAndDrag(clip: Clip, from: DOMRect, x: number, y: number, held: () => boolean) {
  const chip = document.createElement("div");
  chip.className = "flying-chip";
  chip.textContent = `${formatTime(clip.end - clip.start)}  ${clip.title}`;
  document.body.appendChild(chip);
  const box = chip.getBoundingClientRect();
  const dx = from.left + from.width / 2 - (x - box.width / 2);
  const dy = from.top + from.height / 2 - (y - box.height / 2);
  chip.style.left = `${x - box.width / 2}px`;
  chip.style.top = `${y - box.height / 2}px`;
  const flight = chip.animate(
    [
      { transform: `translate(${dx}px, ${dy}px) scale(0.9)`, opacity: 0.6 },
      { transform: "translate(0, 0) scale(1)", opacity: 1 },
    ],
    { duration: FLY_MS, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
  );
  await flight.finished.catch(() => {});
  if (held()) await dragOut(clip);
  chip.remove();
}
