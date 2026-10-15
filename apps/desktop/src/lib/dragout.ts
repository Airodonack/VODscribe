// Dragging a clip out of the window, into a video editor or a folder. A pill
// in the clip bar starts the system's drag once the pointer moves; an Alt-drag
// on a stretch of the timeline (or a press held on the picture) first flies a
// chip from the stretch to the pointer, which shows what's being picked up
// without a word of explanation.

import { api, type Clip } from "./api";
import { ask } from "./ask";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { stretches } from "./media";
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
 * Flies a chip labelled `label` from `from` (the stretch on the timeline) to
 * the pointer at (`x`, `y`), while `clip` is made (or found), then starts the
 * system drag once its file is, if the button is still down (`held`). A file
 * downloaded from a site may take longer than a press: its pill in the clip
 * bar can be dragged when it's done.
 */
export async function flyAndDrag(
  clip: Promise<Clip | null>,
  label: string,
  from: DOMRect,
  x: number,
  y: number,
  held: () => boolean,
) {
  const chip = document.createElement("div");
  chip.className = "flying-chip";
  chip.textContent = label;
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
  try {
    const [made] = await Promise.all([clip, flight.finished.catch(() => {})]);
    if (!made) return;
    chip.textContent = `${formatTime(made.end - made.start)}  ${made.title}`;
    if (made.status !== "ready") chip.classList.add("making");
    const ready = await clips.ready(made.id, held);
    if (ready && held()) await dragOut(ready);
  } finally {
    chip.remove();
  }
}

/** The stretch between marks that `time` is in, of a VOD `duration` seconds long. */
export function stretchAt(time: number, duration: number): { start: number; end: number } | undefined {
  return stretches(
    cuts.list.map((m) => m.start),
    duration,
  ).find((s) => time >= s.start && time < s.end);
}

/** A download from a site longer than this asks first. */
const LONG = 10 * 60;

/** Makes the stretch `start`..`end` a clip (or gives the one made), asking first before a long download. */
export async function exportStretch(start: number, end: number): Promise<Clip | null> {
  if (!clips.find(start, end) && clips.fromSite && end - start > LONG) {
    const sure = await ask(
      `This stretch is ${formatTime(end - start)} long, and it's downloaded from the site to make the clip. Make it?`,
      "Make the clip",
      "Cancel",
    );
    if (!sure) return null;
  }
  return clips.make(start, end);
}

/** Where `start`..`end` is on screen: on the timeline if it's in view, else nowhere useful (null). */
export function rectOf(start: number, end: number): DOMRect | null {
  const bar = document.querySelector<HTMLElement>(".scrubber");
  if (!bar) return null;
  const box = bar.getBoundingClientRect();
  const viewStart = Number(bar.dataset.viewStart);
  const span = Number(bar.dataset.span);
  if (!(span > 0)) return null;
  const left = box.left + ((Math.max(start, viewStart) - viewStart) / span) * box.width;
  const right = box.left + ((Math.min(end, viewStart + span) - viewStart) / span) * box.width;
  return right > left ? new DOMRect(left, box.top, right - left, box.height) : null;
}
