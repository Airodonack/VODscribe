// Carrying a stretch of the timeline (or a clip's pill) somewhere: an
// Alt-drag on the timeline (Option on a Mac), a press held
// on the picture, or a pill dragged from the clip bar. Inside the window it's
// the app's own drag, showing the stretch's frame over its waveform. It lands
// on the clip bar (kept as a clip), on an edit's row in the library's list
// (a copy of the stretch put at the end of that edit; no clip), or on "Make
// new edit" atop the list (an edit made with it); dropped anywhere else,
// nothing happens. Taken past the window's edge, it becomes the system's drag
// of the clip's file, into a video editor or a folder: a clip without a pill,
// forgotten once the drag's over (the file is the other app's then). Its file
// is made as the carry begins where that's quick (the VOD plays from a file
// here), else at the edge, and the drag starts once it's there, if the button
// is still down. Brought back over the window, the system's drag lands where
// a carry would (`backOver`, `backDropped`): on the clip bar it keeps the clip.

import { api, events, thumbUrl, type Clip, type ClipFormat, type Put, type Vod } from "./api";
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

/** The files of the clips being dragged out, so the library doesn't add
 *  them if they're dropped back on the window (Library.svelte). */
let outgoing: string[] = [];
let forget: ReturnType<typeof setTimeout> | undefined;

/** While a clip's file is being dragged out (until the system's drag is
 *  over): brought back over the window, it isn't something dropped in to add. */
let out = false;
let drags = 0;
export const draggingOut = () => out;

/** Whether `paths` are just the clips being dragged out. */
export function isOutgoing(paths: string[]): boolean {
  return outgoing.length > 0 && paths.length > 0 && paths.every((p) => outgoing.includes(p));
}

/** Forgets `clip` if it was made only to be dragged out (no pill in the clip
 *  bar); its file stays, if it was handed out. */
function forgetHidden(clip: Clip | null | undefined) {
  if (clip && !clip.shown) api.deleteClip(clip.id).catch((e) => console.error("forget dragged clip", e));
}

/** Drags the files of clips `list` out of the window together, if they're
 *  all made (pills on an edit's clip bar, one or several picked). */
export const dragFileOut = (list: Clip[]) => dragOut(list);

/**
 * Starts the system drag of the files of clips `list` (one, or several in
 * one drag), if they're all made, showing `image` (a PNG) if given; while
 * `held()`, the button is still down. True if the drag was asked for (then
 * its end, "drag-ended", forgets a clip made only for it); false if the
 * button was let go first, when nothing is asked and the clips are the
 * caller's to forget.
 */
async function dragOut(list: Clip[], image?: Uint8Array, held: () => boolean = () => true): Promise<boolean> {
  if (!list.length || list.some((c) => c.status !== "ready" || !c.file_path)) return false;
  outgoing = list.map((c) => c.file_path!);
  clearTimeout(forget);
  forget = setTimeout(() => (outgoing = []), 60_000);
  out = true;
  // Over a moment after the system says so (events about the drag can come
  // just after), or at the latest in a minute; unless another's begun.
  const mine = ++drags;
  let stop: (() => void) | undefined;
  const over = () => {
    clearTimeout(limit);
    stop?.();
    // Made only for this: forgotten (the file stays, if it was handed out);
    // unless it was brought back and dropped on the clip bar, which keeps it.
    if (!back?.kept) list.forEach(forgetHidden);
    back = null;
    backLeft();
    setTimeout(() => mine === drags && (out = false), 1000);
  };
  const limit = setTimeout(over, 60_000);
  stop = await events.dragEnded(over).catch(() => undefined);
  // Let go while that was set up (the release was already on its way when
  // the edge was crossed): a drag begun now, with no button down, would
  // never end. Nothing begun; undone as if never asked.
  if (!held()) {
    clearTimeout(limit);
    stop?.();
    backLeft();
    if (mine === drags) out = false;
    return false;
  }
  performance.mark("drag-start");
  await api
    .dragClips(
      list.map((c) => c.id),
      image,
    )
    .catch((e) => {
      console.error("drag", e);
      over();
    });
  formatHint.dragged();
  return true;
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

/**
 * Makes each of `ranges` a clip in the clip bar (several stretches
 * selected), in their order, as `exportStretch` makes one: asking first
 * before a long download, all of them at once.
 */
export async function exportStretches(ranges: { start: number; end: number }[]): Promise<void> {
  const fresh = ranges.filter((r) => !clips.find(r.start, r.end));
  const length = fresh.reduce((sum, r) => sum + r.end - r.start, 0);
  if (clips.fromSite && length > LONG) {
    const sure = await ask(
      `These stretches are ${formatTime(length)} long together, and they're downloaded from the site to make the clips. Make them?`,
      "Make the clips",
      "Cancel",
    );
    if (!sure) return;
  }
  await clips.makeAll(ranges);
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

/** Where a drop on the open edit's timeline goes: before its `index`th piece (of its top level), at `time` on it. */
export type Insertion = { index: number; time: number };

/** The open edit's timeline as a place to drop on (EditView.svelte): where a drop at `x` goes. */
let timeline: { el: HTMLElement; at: (x: number) => Insertion } | null = null;

/** Makes `node` (the open edit's timeline) a place to drop on: `at` says where a drop at x goes. */
export function dropsInEdit(node: HTMLElement, at: (x: number) => Insertion) {
  timeline = { el: node, at };
  return {
    update(next: (x: number) => Insertion) {
      if (timeline?.el === node) timeline.at = next;
    },
    destroy() {
      if (timeline?.el === node) timeline = null;
    },
  };
}

/** What's under (`px`, `py`) to drop on: an edit's row, "Make new edit", the open edit's timeline, or the clip bar. */
type Target = { edit: number | "new" | null; timeline: Insertion | null; bar: boolean };

function targetAt(px: number, py: number): Target {
  const el = document.elementFromPoint(px, py);
  const none = { edit: null, timeline: null, bar: false };
  const edit = el?.closest<HTMLElement>("[data-edit-drop]");
  if (edit) return { ...none, edit: Number(edit.dataset.editDrop) };
  if (el?.closest("[data-new-edit]")) return { ...none, edit: "new" };
  if (el && timeline?.el.contains(el) && edits.openId !== null) return { ...none, timeline: timeline.at(px) };
  return { ...none, bar: !!el?.closest(".clip-bar") };
}

/** Shows what's under the pointer, `target`, as where it would drop (EditRow, the
 *  new-edit row, the clip bar, the marker on the open edit's timeline). */
function showTarget(target: Target) {
  clips.overBar = target.bar;
  edits.dropTarget = target.edit;
  edits.inserting = target.timeline?.time ?? null;
}

/** Puts `stretch` where it was dropped (`targetAt`): in an edit, a new edit, on
 *  the open edit's timeline, or the clip bar (a VOD's: `clip` makes it a clip
 *  there; an edit's takes a clip of it); false if nothing was there. */
function dropAt(px: number, py: number, stretch: Stretch, clip: () => unknown): boolean {
  const { edit, timeline, bar } = targetAt(px, py);
  const { vodId, start, end, title } = stretch;
  if (edit === "new") edits.createWith(stretch);
  else if (edit !== null) edits.addRange(edit, stretch);
  else if (timeline && edits.openId !== null) edits.addRange(edits.openId, stretch, timeline.index);
  else if (bar && clips.doc?.kind === "edit")
    edits.put(clips.doc.id, [{ kind: "range", vodId, start, end, title }], true);
  else if (bar) clip();
  return edit !== null || timeline !== null || bar;
}

/** What's being dragged out as files, while it is: brought back over the
 *  window, it can still go where a carry inside could (`over` lights up
 *  what's under the pointer, `dropped` puts it there). `kept` once a clip
 *  made for the drag is kept, dropped on the clip bar. */
let back: { over: (x: number, y: number) => void; dropped: (x: number, y: number) => void; kept?: boolean } | null =
  null;

/** The system's drag of a clip out came back over the window, at (`x`, `y`)
 *  in its CSS pixels: what's under it lights up. */
export function backOver(x: number, y: number) {
  back?.over(x, y);
}

/** It left the window again, or ended. */
export function backLeft() {
  clips.carrying = null;
  clips.carried = 0;
  showTarget({ edit: null, timeline: null, bar: false });
}

/** It was dropped on the window, at (`x`, `y`): put where it was dropped. */
export function backDropped(x: number, y: number) {
  backLeft();
  back?.dropped(x, y);
}

/** Brought back over the window, the stretch `stretch` goes where a carry of it would; `clip` keeps it as a clip. */
function bringsBack(stretch: Stretch, clip: () => unknown) {
  const b: NonNullable<typeof back> = {
    over(x, y) {
      // The drop targets show as for a carry (EditRow, the new-edit row).
      clips.carrying = { start: stretch.start, end: stretch.end };
      showTarget(targetAt(x, y));
    },
    dropped(x, y) {
      dropAt(x, y, stretch, () => {
        b.kept = true;
        return clip();
      });
    },
  };
  return b;
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
  let pictureUrl: string | null = null;
  const picture = pictureOf(vod, start, ownPlayer).then((p) => {
    if (p?.own) pictureUrl = p.url;
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
      early?.then(forgetHidden);
      early = null;
      window.removeEventListener("pointermove", onMove, true);
      window.removeEventListener("pointerup", onUp, true);
      window.removeEventListener("keydown", onKey, true);
      clips.carrying = null;
      showTarget({ edit: null, timeline: null, bar: false });
      document.body.classList.remove("timeline-drag");
      ghost.remove();
      if (pictureUrl) URL.revokeObjectURL(pictureUrl);
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
      showTarget(targetAt(e.clientX, e.clientY));
      const outside = e.clientX <= 0 || e.clientY <= 0 || e.clientX >= innerWidth - 1 || e.clientY >= innerHeight - 1;
      if (outside) takeOut();
    };
    const onUp = (e: PointerEvent) => {
      down = false;
      if (leaving) return;
      // A drop on the clip bar keeps the clip made ahead (the same stretch):
      // it's waited for, then shown. Anywhere else it goes, as in finish.
      const ahead = early;
      early = null;
      let kept = false;
      finish();
      dropAt(e.clientX, e.clientY, stretch, () => {
        kept = true;
        return (ahead ?? Promise.resolve(null)).then(() => make(true));
      });
      if (!kept) ahead?.then(forgetHidden);
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
        const cut = made && (await clips.ready(made.id, () => down, vod.id));
        const ready = cut && (await inFormat(cut, vod.id, () => down));
        // Its file can be ready as the edge is crossed (a short clip from a
        // file here, on a slow machine); the release then arrives during the
        // waits below, so the button is checked again before the drag is
        // asked for (dragOut), not only here.
        const png = ready && down ? await image : null;
        if (ready && down) {
          back = bringsBack(stretch, () => make(true));
          dragged = await dragOut([ready], png ?? undefined, () => down);
          if (!dragged) back = null;
        }
      } finally {
        finish();
        // Let go before its file was ready, or before the drag began: the
        // clip made only for the drag goes (dragOut forgets it, after a drag).
        if (!dragged) forgetHidden(made);
      }
    };
    window.addEventListener("pointermove", onMove, true);
    window.addEventListener("pointerup", onUp, true);
    window.addEventListener("keydown", onKey, true);
  });
}

/** Several things picked (selection.svelte.ts), carried together (`carrySet`). */
export interface CarrySet {
  /** What they are, as an edit takes them, in the order picked. */
  items: Put[];
  /** What the ghost says: "3 clips". */
  label: string;
  /** The open VOD's stretches among them: its clip bar keeps them as clips. */
  stretches: { start: number; end: number }[];
  /** Makes the clips whose files go out past the window's edge (one only for
   *  the drag isn't in the clip bar); none for rows of the library. */
  files?: () => Promise<Clip[]>;
  /** The open VOD, if it's its: the files are made in the format clips are made in. */
  vodId?: number;
  /** Where on screen the item pressed is, for the ghost to fly from. */
  from: DOMRect;
  x: number;
  y: number;
}

/** What of `set` the place under the pointer, `target`, takes into an edit:
 *  on the open edit's timeline, not its own stretches (that would copy them
 *  in place), nor an edit into itself. (A VOD's clip bar takes its stretches.) */
function takes(set: CarrySet, target: Target): Put[] {
  const into = target.edit === "new" ? null : target.timeline ? edits.openId : target.edit;
  const own = (p: Put) => (p.kind === "clip" && p.id === into) || (!!target.timeline && p.kind === "span");
  if (target.edit !== null || target.timeline) return set.items.filter((p) => !own(p));
  return target.bar && clips.doc?.kind === "edit" ? set.items : [];
}

/** What under (`px`, `py`) takes something of `set`; nothing, if nothing would go there. */
function setTargetAt(set: CarrySet, px: number, py: number): Target {
  const target = targetAt(px, py);
  const vodBar = target.bar && clips.doc?.kind === "vod";
  return (vodBar ? set.stretches.length : takes(set, target).length)
    ? target
    : { edit: null, timeline: null, bar: false };
}

/** Puts `set` where it was dropped, at (`px`, `py`), as one change; false if nothing took it. */
function dropSet(set: CarrySet, px: number, py: number): boolean {
  const target = setTargetAt(set, px, py);
  const items = takes(set, target);
  if (target.edit === "new") edits.createWith(items);
  else if (target.edit !== null) edits.put(target.edit, items, false);
  else if (target.timeline && edits.openId !== null) edits.put(edits.openId, items, false, target.timeline.index);
  else if (target.bar && clips.doc?.kind === "edit") edits.put(clips.doc.id, items, true);
  else if (target.bar) void exportStretches(set.stretches);
  return target.edit !== null || target.timeline !== null || target.bar;
}

/**
 * Carries several things picked from (`x`, `y`) while the button is down, as
 * `carry` carries one: onto an edit's row (at the end of its timeline), "Make
 * new edit", the open edit's timeline (where the marker shows) or the clip
 * bar (clips there), all in the order picked, as one change. Past the
 * window's edge, the system's drag of all their files, once they're made
 * (rows of the library have none: the carry ends there). Resolves once it's
 * dropped, taken out of the window, or let go.
 */
export function carrySet(set: CarrySet): Promise<void> {
  const ghost = document.createElement("div");
  ghost.className = "carry-ghost";
  const caption = document.createElement("span");
  caption.textContent = set.label;
  ghost.append(caption);
  document.body.appendChild(ghost);
  const place = (px: number, py: number) => {
    ghost.style.left = `${px + 14}px`;
    ghost.style.top = `${py + 14}px`;
  };
  place(set.x, set.y);
  ghost.animate(
    [
      {
        transform: `translate(${set.from.left + set.from.width / 2 - set.x - 14}px, ${set.from.top - set.y - 14}px) scale(0.6)`,
        opacity: 0.5,
      },
      { transform: "translate(0, 0) scale(1)", opacity: 1 },
    ],
    { duration: FLY_MS, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
  );
  clips.carried = set.items.length;
  document.body.classList.add("timeline-drag");
  window.getSelection()?.removeAllRanges();

  return new Promise((resolve) => {
    let down = true;
    let leaving = false;
    const finish = () => {
      window.removeEventListener("pointermove", onMove, true);
      window.removeEventListener("pointerup", onUp, true);
      window.removeEventListener("keydown", onKey, true);
      clips.carried = 0;
      showTarget({ edit: null, timeline: null, bar: false });
      document.body.classList.remove("timeline-drag");
      ghost.remove();
      resolve();
    };
    const onMove = (e: PointerEvent) => {
      if (leaving) return;
      place(e.clientX, e.clientY);
      showTarget(setTargetAt(set, e.clientX, e.clientY));
      const outside = e.clientX <= 0 || e.clientY <= 0 || e.clientX >= innerWidth - 1 || e.clientY >= innerHeight - 1;
      if (outside) takeOut();
    };
    const onUp = (e: PointerEvent) => {
      down = false;
      if (leaving) return;
      finish();
      dropSet(set, e.clientX, e.clientY);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !leaving) {
        e.preventDefault();
        finish();
      }
    };
    // Past the window's edge: the system's drag of all their files, once
    // each is made (and in the format clips are made in), if the button's
    // still down by then.
    const takeOut = async () => {
      leaving = true;
      if (!set.files) return finish();
      performance.mark("carry-out");
      ghost.classList.add("making");
      let made: Clip[] = [];
      let dragged = false;
      try {
        made = await set.files();
        const ready: Clip[] = [];
        for (const clip of made) {
          const cut = down ? await clips.ready(clip.id, () => down) : null;
          const done = cut && set.vodId !== undefined ? await inFormat(cut, set.vodId, () => down) : cut;
          if (!done) break;
          ready.push(done);
        }
        if (made.length && ready.length === made.length && down) {
          const blank = document.createElement("canvas");
          blank.width = PICTURE_WIDTH;
          blank.height = WAVE_HEIGHT;
          const png = await snapshot(null, blank, set.label);
          back = bringsBackSet(set);
          dragged = await dragOut(ready, png, () => down);
          if (!dragged) back = null;
        }
      } finally {
        finish();
        // Let go before their files were ready, or before the drag began:
        // the clips made only for the drag go (dragOut forgets them, after one).
        if (!dragged) made.forEach(forgetHidden);
      }
    };
    window.addEventListener("pointermove", onMove, true);
    window.addEventListener("pointerup", onUp, true);
    window.addEventListener("keydown", onKey, true);
  });
}

/** Brought back over the window, `set`'s system drag goes where a carry of it would. */
function bringsBackSet(set: CarrySet) {
  const b: NonNullable<typeof back> = {
    over(x, y) {
      clips.carried = set.items.length;
      showTarget(setTargetAt(set, x, y));
    },
    dropped(x, y) {
      // On a VOD's clip bar, the clips made for the drag are the ones kept.
      if (setTargetAt(set, x, y).bar && clips.doc?.kind === "vod") b.kept = true;
      dropSet(set, x, y);
    },
  };
  return b;
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
