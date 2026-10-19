// Carrying a stretch of the timeline (or a clip's pill) somewhere: an
// Alt-drag on the timeline (Option on a Mac), a press held
// on the picture, or a pill dragged from the clip bar. Inside the window it's
// the app's own drag, showing the stretch's frame over its waveform. It lands
// on the clip bar (kept as a clip), on an edit's row in the library's list
// (a copy of the stretch put at the end of that edit; no clip), or on "Make
// new edit" atop the list (an edit made with it); dropped anywhere else,
// nothing happens. Taken past the window's edge, it becomes the system's drag
// of the clip's file, into a video editor or a folder: the file is made then
// (without a pill, and forgotten once the drag's over: the file is the other
// app's then), and the drag starts once it's there, if the button is still down.

import { api, events, thumbUrl, type Clip, type ClipFormat, type Vod } from "./api";
import { settings } from "./appSettings.svelte";
import { formatHint } from "./resolveHint.svelte";
import { ask } from "./ask";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { edits, type Stretch } from "./edits.svelte";
import { PEAKS_RATE, columns, stretchIn } from "./media";
import { formatTime } from "./platform";

/** How long a press is held before it picks the clip up. */
export const HOLD_MS = 250;
/** How far, in pixels, a press moves before it's a drag. */
export const DRAG_SLOP = 4;
const FLY_MS = 150;

/** The file of the clip being dragged out, so the library doesn't add it if
 *  it's dropped back on the window (Library.svelte). */
let outgoing: string | null = null;
let forget: ReturnType<typeof setTimeout> | undefined;

/** While a clip's file is being dragged out (until the system's drag is
 *  over): brought back over the window, it isn't something dropped in to add. */
let out = false;
let drags = 0;
export const draggingOut = () => out;

/** Whether `paths` are just the clip being dragged out. */
export function isOutgoing(paths: string[]): boolean {
  return outgoing !== null && paths.length > 0 && paths.every((p) => p === outgoing);
}

/** Starts the system drag of `clip`, if its file is made, showing `image` (a PNG) if given. */
async function dragOut(clip: Clip, image?: Uint8Array): Promise<void> {
  if (clip.status !== "ready" || !clip.file_path) return;
  outgoing = clip.file_path;
  clearTimeout(forget);
  forget = setTimeout(() => (outgoing = null), 60_000);
  out = true;
  // Over a moment after the system says so (events about the drag can come
  // just after), or at the latest in a minute; unless another's begun.
  const mine = ++drags;
  let stop: (() => void) | undefined;
  const over = () => {
    clearTimeout(limit);
    stop?.();
    // Made only for this: forgotten (the file stays, if it was handed out).
    if (!clip.shown) api.deleteClip(clip.id).catch(() => {});
    back = null;
    backLeft();
    setTimeout(() => mine === drags && (out = false), 1000);
  };
  const limit = setTimeout(over, 60_000);
  stop = await events.dragEnded(over).catch(() => undefined);
  performance.mark("drag-start");
  await api.dragClip(clip.id, image).catch((e) => {
    console.error("drag", e);
    over();
  });
  formatHint.dragged();
}

/** The format clip `clip`'s file is in, by its extension. */
const formatOf = (clip: Clip): ClipFormat => (/\.(mov|wav)$/i.test(clip.file_path ?? "") ? "mov" : "mp4");

/** Clip `clip` (of VOD `vodId`) in the format clips are made in now: made
 *  again if it's in the other, while `still()`. */
async function inFormat(clip: Clip, vodId: number, still: () => boolean): Promise<Clip | null> {
  await settings.load();
  const want = settings.value?.clip_format ?? "mp4";
  if (formatOf(clip) === want) return clip;
  if (clips.vodId === vodId) await clips.move(clip.id, clip.start, clip.end);
  else if (!(await api.setClipRange(clip.id, clip.start, clip.end).catch(() => null))) return null;
  return clips.ready(clip.id, still, vodId);
}

/** The stretch between marks that `time` is in, of a VOD `duration` seconds long. */
export function stretchAt(time: number, duration: number): { start: number; end: number } | undefined {
  return stretchIn(
    cuts.list.map((m) => m.start),
    duration,
    time,
  );
}

/** A download from a site longer than this asks first. */
const LONG = 10 * 60;

/**
 * Makes the stretch `start`..`end` a clip (or gives the one made), asking
 * first before a long download; in the clip bar if `shown`, else only made to
 * be dragged out.
 */
export async function exportStretch(start: number, end: number, shown = true): Promise<Clip | null> {
  if (!clips.find(start, end) && clips.fromSite && end - start > LONG) {
    const sure = await ask(
      `This stretch is ${formatTime(end - start)} long, and it's downloaded from the site to make the clip. Make it?`,
      "Make the clip",
      "Cancel",
    );
    if (!sure) return null;
  }
  return clips.make(start, end, shown);
}

/** Where `start`..`end` is on screen: on the timeline if it's in view, else null. */
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

const PICTURE_WIDTH = 160;
const WAVE_HEIGHT = 28;

/** A picture of the stretch: its first frame (or the VOD's thumbnail), as a URL; null if there's none.
 *  `own` if it's made here, to be revoked when done (a thumbnail's is shared, api.ts). */
async function pictureOf(vod: Vod, time: number, ownPlayer: boolean): Promise<{ url: string; own: boolean } | null> {
  if (ownPlayer && !vod.audio_only) {
    const jpeg = await api.frame(vod.id, time).catch(() => new Uint8Array());
    if (jpeg.length)
      return { url: URL.createObjectURL(new Blob([jpeg as BlobPart], { type: "image/jpeg" })), own: true };
  }
  const url = vod.thumb ? await thumbUrl(vod.thumb).catch(() => null) : null;
  return url ? { url, own: false } : null;
}

/** The stretch's waveform, drawn on `canvas`. */
function drawWave(canvas: HTMLCanvasElement, levels: Uint8Array, start: number, end: number) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const shown = levels.subarray(Math.floor(start * PEAKS_RATE), Math.ceil(end * PEAKS_RATE) + 1);
  const bars = columns(shown, end - start, Math.floor(canvas.width / 2));
  ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#4f46e5";
  for (let i = 0; i < bars.length; i++) {
    const h = Math.max(1, bars[i] * canvas.height);
    ctx.fillRect(i * 2, (canvas.height - h) / 2, 1, h);
  }
}

/** The ghost's picture and waveform as one PNG, for the system's drag. */
async function snapshot(
  img: HTMLImageElement | null,
  wave: HTMLCanvasElement,
  label: string,
): Promise<Uint8Array | undefined> {
  const pictureHeight = img?.naturalWidth ? Math.round((img.naturalHeight / img.naturalWidth) * PICTURE_WIDTH) : 0;
  const canvas = document.createElement("canvas");
  canvas.width = PICTURE_WIDTH;
  canvas.height = pictureHeight + WAVE_HEIGHT + 18;
  const ctx = canvas.getContext("2d");
  if (!ctx) return undefined;
  ctx.fillStyle = "#1c1c1b";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  if (img && pictureHeight) ctx.drawImage(img, 0, 0, PICTURE_WIDTH, pictureHeight);
  ctx.drawImage(wave, 0, pictureHeight);
  ctx.fillStyle = "#ecece9";
  ctx.font = "12px sans-serif";
  ctx.fillText(label, 6, pictureHeight + WAVE_HEIGHT + 13, PICTURE_WIDTH - 12);
  const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, "image/png"));
  return blob ? new Uint8Array(await blob.arrayBuffer()) : undefined;
}

/** What's under (`px`, `py`) to drop a stretch on: an edit, "Make new edit", or the clip bar. */
function targetAt(px: number, py: number): { edit: number | "new" | null; bar: boolean } {
  const el = document.elementFromPoint(px, py);
  const edit = el?.closest<HTMLElement>("[data-edit-drop]");
  if (edit) return { edit: Number(edit.dataset.editDrop), bar: false };
  if (el?.closest("[data-new-edit]")) return { edit: "new", bar: false };
  return { edit: null, bar: !!el?.closest(".clip-bar") };
}

/** Puts `stretch` where it was dropped (`targetAt`): in an edit, a new edit, or
 *  the clip bar (`clip` makes it a clip there); false if nothing was there. */
function dropAt(px: number, py: number, stretch: Stretch, clip: () => unknown): boolean {
  const { edit, bar } = targetAt(px, py);
  if (edit === "new") edits.createWith(stretch);
  else if (edit !== null) edits.addRange(edit, stretch);
  else if (bar) clip();
  return edit !== null || bar;
}

/** The stretch whose clip is being dragged out as a file, while it is: brought
 *  back over the window, it can still go where a carry inside could. */
let back: { stretch: Stretch; clip: () => unknown } | null = null;

/** The system's drag of a clip out came back over the window, at (`x`, `y`)
 *  in its CSS pixels: what's under it lights up. */
export function backOver(x: number, y: number) {
  if (!back) return;
  // The drop targets show as for a carry (EditRow, the new-edit row).
  clips.carrying = { start: back.stretch.start, end: back.stretch.end };
  const target = targetAt(x, y);
  clips.overBar = target.bar;
  edits.dropTarget = target.edit;
}

/** It left the window again, or ended. */
export function backLeft() {
  clips.carrying = null;
  clips.overBar = false;
  edits.dropTarget = null;
}

/** It was dropped on the window, at (`x`, `y`): put where it was dropped. */
export function backDropped(x: number, y: number) {
  backLeft();
  if (back) dropAt(x, y, back.stretch, back.clip);
}

export interface Carry {
  vod: Vod;
  start: number;
  end: number;
  /** Whether the VOD plays in the app's own player (its frames can be read). */
  ownPlayer: boolean;
  /** The VOD's waveform (`peaks`), if it has one. */
  levels?: Uint8Array;
  /** Where on screen the stretch (or pill) is, for the ghost to fly from. */
  from: DOMRect;
  x: number;
  y: number;
  /** Its name, if it's a clip's: a copy put in an edit keeps it. */
  title?: string;
  /** Where it came from, if coming back there ends the carry (an edit's timeline). */
  home?: CarryHome;
}

/** Where a carry came from: back over `el`, it ends and `back` takes the press on, at its x. */
export interface CarryHome {
  el: HTMLElement;
  back: (x: number, y: number) => void;
}

/**
 * Carries a stretch from (`x`, `y`) while the button is down: see the top of
 * this file. Resolves once it's dropped, taken out of the window, or let go.
 */
export function carry({ vod, start, end, ownPlayer, levels, from, x, y, title, home }: Carry): Promise<void> {
  // A stretch of the VOD shown is made a clip through its clip bar; another's (an edit's piece) directly.
  const make = async (shown: boolean): Promise<Clip | null> => {
    if (vod.id === clips.vodId) return exportStretch(start, end, shown);
    return api.addClip(vod.id, start, end, shown).catch((e) => {
      clips.error = String(e);
      return null;
    });
  };
  const label = `${formatTime(end - start)}  ${formatTime(start)}–${formatTime(end)}`;
  const stretch: Stretch = { vodId: vod.id, start, end, title };
  const ghost = document.createElement("div");
  ghost.className = "carry-ghost";
  const img = document.createElement("img");
  img.alt = "";
  img.width = PICTURE_WIDTH;
  const wave = document.createElement("canvas");
  wave.width = PICTURE_WIDTH;
  wave.height = WAVE_HEIGHT;
  const waveform = levels ? Promise.resolve(levels) : api.peaks(vod.id).catch(() => new Uint8Array());
  const drawn = waveform.then((l) => void (l.length && drawWave(wave, l, start, end)));
  const caption = document.createElement("span");
  caption.textContent = label;
  ghost.append(img, wave, caption);
  document.body.appendChild(ghost);
  let made: string | null = null;
  const picture = pictureOf(vod, start, ownPlayer).then((p) => {
    if (p?.own) made = p.url;
    if (p) img.src = p.url;
    else img.remove();
    return p
      ? img.decode().then(
          () => img,
          () => null,
        )
      : null;
  });

  // What the system's drag needs, made while the carry goes on rather than
  // once it's out of the window, where nothing can be dropped until both are
  // there: the picture (as soon as its parts are drawn), and the clip's file
  // where cutting it is quick (from a file here: copied, not downloaded).
  const image = Promise.all([picture, drawn]).then(([img]) => snapshot(img, wave, label));
  let early: Promise<Clip | null> | null = ownPlayer ? make(false) : null;

  const place = (px: number, py: number) => {
    ghost.style.left = `${px + 14}px`;
    ghost.style.top = `${py + 14}px`;
  };
  place(x, y);
  ghost.animate(
    [
      {
        transform: `translate(${from.left + from.width / 2 - x - 14}px, ${from.top - y - 14}px) scale(0.6)`,
        opacity: 0.5,
      },
      { transform: "translate(0, 0) scale(1)", opacity: 1 },
    ],
    { duration: FLY_MS, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
  );
  clips.carrying = { start, end };
  document.body.classList.add("timeline-drag");
  // Text the press began selecting (a pill's name) isn't carried along.
  window.getSelection()?.removeAllRanges();

  return new Promise((resolve) => {
    let down = true;
    let leaving = false;
    const finish = () => {
      // Made ahead for a drag out that didn't come: it goes.
      early?.then((c) => {
        if (c && !c.shown) api.deleteClip(c.id).catch(() => {});
      });
      early = null;
      window.removeEventListener("pointermove", onMove, true);
      window.removeEventListener("pointerup", onUp, true);
      window.removeEventListener("keydown", onKey, true);
      clips.carrying = null;
      clips.overBar = false;
      edits.dropTarget = null;
      document.body.classList.remove("timeline-drag");
      ghost.remove();
      if (made) URL.revokeObjectURL(made);
      resolve();
    };
    const onMove = (e: PointerEvent) => {
      if (leaving) return;
      const box = home?.el.getBoundingClientRect();
      if (
        home &&
        box &&
        e.clientX >= box.left &&
        e.clientX <= box.right &&
        e.clientY >= box.top &&
        e.clientY <= box.bottom
      ) {
        finish();
        home.back(e.clientX, e.clientY);
        return;
      }
      place(e.clientX, e.clientY);
      const target = targetAt(e.clientX, e.clientY);
      clips.overBar = target.bar;
      edits.dropTarget = target.edit;
      const outside = e.clientX <= 0 || e.clientY <= 0 || e.clientX >= innerWidth - 1 || e.clientY >= innerHeight - 1;
      if (outside) takeOut();
    };
    const onUp = (e: PointerEvent) => {
      down = false;
      if (leaving) return;
      finish();
      dropAt(e.clientX, e.clientY, stretch, () => make(true));
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !leaving) {
        e.preventDefault();
        finish();
      }
    };
    // Past the window's edge: the system's drag of the clip's file, made for it.
    const takeOut = async () => {
      leaving = true;
      // From here to "drag-start" is what's waited at the edge (tests time it).
      performance.mark("carry-out");
      clips.overBar = false;
      ghost.classList.add("making");
      let made: Clip | null = null;
      let dragged = false;
      const ahead = early;
      early = null;
      try {
        made = await (ahead ?? make(false));
        const made2 = made && (await clips.ready(made.id, () => down, vod.id));
        const ready = made2 && (await inFormat(made2, vod.id, () => down));
        if (ready && down) {
          dragged = true;
          back = { stretch, clip: () => make(true) };
          await dragOut(ready, await image);
        }
      } finally {
        finish();
        // Let go before its file was ready: the clip made only for the drag
        // goes (dragOut forgets it, after a drag).
        if (made && !made.shown && !dragged) api.deleteClip(made.id).catch(() => {});
      }
    };
    window.addEventListener("pointermove", onMove, true);
    window.addEventListener("pointerup", onUp, true);
    window.addEventListener("keydown", onKey, true);
  });
}

/**
 * A press held still on a player's picture carries the stretch between cuts
 * that the playhead is in, flying from the timeline to the pointer; a press
 * let go first is a click, which plays or pauses (`toggle`). The picture's
 * pointer handlers, for a player of `vod()` whose time is `time()` in a VOD
 * `length()` long, its picture `box()`; `stop()` when the player goes.
 */
export function holdToCarry(
  vod: () => Vod,
  time: () => number,
  length: () => number,
  box: () => HTMLElement | undefined,
  toggle: () => void,
) {
  let hold: ReturnType<typeof setTimeout> | undefined;
  let held = false;
  let pointer = { x: 0, y: 0 };
  const stop = () => clearTimeout(hold);
  return {
    down(e: PointerEvent) {
      if (e.button !== 0) return;
      const range = stretchAt(time(), length());
      held = false;
      pointer = { x: e.clientX, y: e.clientY };
      if (!range || !cuts.list.length) return;
      hold = setTimeout(() => {
        held = true;
        const from = rectOf(range.start, range.end) ?? box()?.getBoundingClientRect();
        if (from)
          carry({ vod: vod(), start: range.start, end: range.end, ownPlayer: true, from, x: pointer.x, y: pointer.y });
      }, HOLD_MS);
    },
    move(e: PointerEvent) {
      if (Math.hypot(e.clientX - pointer.x, e.clientY - pointer.y) >= DRAG_SLOP) stop();
      pointer = { x: e.clientX, y: e.clientY };
    },
    up: stop,
    click() {
      // A press that picked a clip up isn't a click.
      if (held) held = false;
      else toggle();
    },
    stop,
  };
}
