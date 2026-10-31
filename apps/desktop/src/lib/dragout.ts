// Carrying a stretch of the timeline (or a clip's pill) somewhere: an
// Alt-drag on the timeline (Option on a Mac), a press held
// on the picture, a pill dragged from the clip bar, or a highlight's row
// dragged from the annotations pane (into an edit, a group named for it). Inside the window it's
// the app's own drag, showing the stretch's frame over its waveform. It lands
// on the clip bar (kept as a clip), on an edit's row in the library's list
// (a copy of the stretch put at the end of that edit; no clip), or on "Make
// new edit" atop the list (an edit made with it); dropped anywhere else,
// nothing happens. Taken past the window's edge, it becomes the system's drag
// of the clip's file, into a video editor or a folder: a clip without a pill,
// forgotten once the drag's over (the file is the other app's then). Its file
// (each one's, for several picked) is made as the carry begins where that's
// quick (the VOD plays from a file here), first in the queue, else at the
// edge. The system's drag begins at the edge at once, while the button is
// down, the file handed over once it's made (where the system can wait for
// it: `beforeMade`); else it begins once the file's there. Brought back over
// the window, the system's drag lands where a carry would (`backOver`,
// `backDropped`): on the clip bar it keeps the clip.

import { api, events, thumbUrl, type Clip, type ClipFormat, type DragEnded, type Put, type Vod } from "./api";
import { settings } from "./appSettings.svelte";
import { formatHint } from "./resolveHint.svelte";
import { ask } from "./ask";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { edits, putOf, type Stretch } from "./edits.svelte";
import { PEAKS_RATE, columns, stretchIn } from "./media";
import { formatTime, playsFromFile } from "./platform";
import { DRAG_SLOP, holds } from "./pressToDrag";

/** How long a press is held before it picks the clip up. */
export const HOLD_MS = 250;
const FLY_MS = 150;

/** The files of the clips being dragged out, so the library doesn't add
 *  them if they're dropped back on the window (Library.svelte). */
let outgoing: string[] = [];

/** While a clip's file is being dragged out (until the system's drag is
 *  over): brought back over the window, it isn't something dropped in to add. */
let out = false;
let drags = 0;
export const draggingOut = () => out;

/** Whether `paths` are just the clips being dragged out. */
export function isOutgoing(paths: string[]): boolean {
  return outgoing.length > 0 && paths.length > 0 && paths.every((p) => outgoing.includes(p));
}

/** Whether the system's drag can begin before the files are made (the app
 *  hands them over once they are); else it waits for them. */
const beforeMade = () => api.dragBeforeMade().catch(() => false);

/** Forgets `clip` if it was made only to be dragged out (no pill in the clip
 *  bar); its file stays, if it was handed out. */
function forgetHidden(clip: Clip | null | undefined) {
  if (clip && !clip.shown) api.deleteClip(clip.id).catch((e) => console.error("forget dragged clip", e));
}

/**
 * Starts the system drag of the files of clips `list` (one, or several in
 * one drag), made or (`beforeMade`) being made, showing `image` (a PNG) if
 * given; while `held()`, the button is still down. True if the drag was
 * asked for (then its end, "drag-ended", forgets a clip made only for it);
 * false if the button was let go first, when nothing is asked and the clips
 * are the caller's to forget.
 */
async function dragOut(list: Clip[], image?: Uint8Array, held: () => boolean = () => true): Promise<boolean> {
  if (!list.length || list.some((c) => c.status === "error")) return false;
  outgoing = list.filter((c) => c.status === "ready" && c.file_path).map((c) => c.file_path!);
  out = true;
  // Over a moment after the system says so (events about the drag can come
  // just after), unless another's begun. The system says so only once the
  // files went where they were dropped, or couldn't: however long their
  // making takes, they're not forgotten before.
  const mine = ++drags;
  let stop: (() => void) | undefined;
  const over = (ended?: DragEnded) => {
    stop?.();
    if (ended?.failed) clips.error = `Couldn't make the clip: ${ended.failed}`;
    // Made only for this: forgotten (the file stays, if it was handed out);
    // unless it was brought back and dropped on the clip bar, which keeps it.
    if (!back?.kept) list.forEach(forgetHidden);
    back = null;
    backLeft();
    setTimeout(() => {
      if (mine !== drags) return;
      out = false;
      outgoing = [];
    }, 1000);
  };
  stop = await events.dragEnded(over).catch(() => undefined);
  // Let go while that was set up (the release was already on its way when
  // the edge was crossed): a drag begun now, with no button down, would
  // never end. Nothing begun; undone as if never asked.
  if (!held()) {
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
      // Refused: said, unless it's only that the button came up first
      // (clips.rs `start_drag`), a drag that never was.
      if (!/released/.test(String(e))) clips.error = String(e);
      over();
    });
  formatHint.dragged();
  return true;
}

/** The format clip `clip`'s file is in, by its extension. */
const formatOf = (clip: Clip): ClipFormat => (/\.(mov|wav)$/i.test(clip.file_path ?? "") ? "mov" : "mp4");

/** Clip `clip` (of VOD `vodId`) to be in the format clips are made in now:
 *  as it is, or (made in the other) being made again; null if it can't be. */
async function remade(clip: Clip, vodId: number): Promise<Clip | null> {
  await settings.load();
  const want = settings.value?.clip_format ?? "mp4";
  if (clip.status !== "ready" || formatOf(clip) === want) return clip;
  if (clips.vodId === vodId) {
    await clips.move(clip.id, clip.start, clip.end);
    return { ...clip, status: "waiting", file_path: null };
  }
  return api.setClipRange(clip.id, clip.start, clip.end).catch(() => null);
}

/** Clip `clip` (of VOD `vodId`) in the format clips are made in now, once
 *  made (again, if it was in the other), while `still()`. */
async function inFormat(clip: Clip, vodId: number, still: () => boolean): Promise<Clip | null> {
  const want = await remade(clip, vodId);
  return want && (want === clip ? clip : clips.ready(clip.id, still, vodId));
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

/** How a stretch carried is said under its picture: "0:05  1:00–1:05". */
export const stretchSaid = (start: number, end: number) =>
  `${formatTime(end - start)}  ${formatTime(start)}–${formatTime(end)}`;

/** What a ghost's card shows: stretch `start`..`end` of `vod`. */
interface Look {
  vod: Vod;
  start: number;
  end: number;
  /** Whether the VOD plays in the app's own player (its frames can be read). */
  ownPlayer: boolean;
  /** The VOD's waveform, if it's at hand (else it's asked for). */
  levels?: Uint8Array;
}

/**
 * A carry's ghost card: `look`'s first frame over its waveform, with
 * `caption` under them if given (a card behind another has none). Its
 * picture and waveform come in as they're read; `image` resolves once both
 * have (the picture, or null if there's none), and `done` lets go of what
 * was made for it.
 */
function card(look: Promise<Look | null>, caption?: string) {
  const el = document.createElement("div");
  el.className = "carry-ghost";
  const img = document.createElement("img");
  img.alt = "";
  img.width = PICTURE_WIDTH;
  const wave = document.createElement("canvas");
  wave.width = PICTURE_WIDTH;
  wave.height = WAVE_HEIGHT;
  el.append(img, wave);
  if (caption !== undefined) {
    const span = document.createElement("span");
    span.textContent = caption;
    el.append(span);
  }
  const drawn = look.then(async (l) => {
    if (!l) return;
    const levels = l.levels ?? (await api.peaks(l.vod.id).catch(() => new Uint8Array()));
    if (levels.length) drawWave(wave, levels, l.start, l.end);
  });
  let own: string | null = null;
  let gone = false;
  const picture = look
    .then((l) => (l ? pictureOf(l.vod, l.start, l.ownPlayer) : null))
    .then((p) => {
      if (p?.own) {
        // Come after the carry was over: let go of at once.
        if (gone) URL.revokeObjectURL(p.url);
        else own = p.url;
      }
      if (p) img.src = p.url;
      else img.remove();
      return p
        ? img.decode().then(
            () => img,
            () => null,
          )
        : null;
    });
  return {
    el,
    wave,
    image: Promise.all([picture, drawn]).then(([img]) => img),
    done() {
      gone = true;
      if (own) URL.revokeObjectURL(own);
    },
  };
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

/** Nowhere to drop. */
const NOWHERE: Target = { edit: null, timeline: null, bar: false };

function targetAt(px: number, py: number): Target {
  const el = document.elementFromPoint(px, py);
  const edit = el?.closest<HTMLElement>("[data-edit-drop]");
  if (edit) return { ...NOWHERE, edit: Number(edit.dataset.editDrop) };
  if (el?.closest("[data-new-edit]")) return { ...NOWHERE, edit: "new" };
  if (el && timeline?.el.contains(el) && edits.openId !== null) return { ...NOWHERE, timeline: timeline.at(px) };
  return { ...NOWHERE, bar: !!el?.closest(".clip-bar") };
}

/** Whether the pointer is at the window's edge, where a carry goes out as the system's drag. */
const atEdge = (e: PointerEvent) =>
  e.clientX <= 0 || e.clientY <= 0 || e.clientX >= innerWidth - 1 || e.clientY >= innerHeight - 1;

/** Puts a carry's `ghost` on the page beside the pointer at (`x`, `y`),
 *  flying there from `from`; returns what moves it with the pointer. */
function showGhost(ghost: HTMLElement, from: DOMRect, x: number, y: number) {
  const place = (px: number, py: number) => {
    ghost.style.left = `${px + 14}px`;
    ghost.style.top = `${py + 14}px`;
  };
  document.body.appendChild(ghost);
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
  return place;
}

/** Shows what's under the pointer, `target`, as where it would drop (EditRow, the
 *  new-edit row, the clip bar, the marker on the open edit's timeline). */
function showTarget(target: Target) {
  clips.overBar = target.bar;
  edits.dropTarget = target.edit;
  edits.inserting = target.timeline?.time ?? null;
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
  showTarget(NOWHERE);
}

/** It was dropped on the window, at (`x`, `y`): put where it was dropped. */
export function backDropped(x: number, y: number) {
  backLeft();
  back?.dropped(x, y);
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
  /** The clip it is, if it's a pill of the clip bar: an edit takes the clip
   *  itself (a group, its own cuts kept), not a bare stretch. */
  clip?: number;
  /** Whether it's a highlight (a row of the annotations pane): an edit takes
   *  it as a group named for it, one band as a clip's pill is. */
  highlight?: boolean;
  /** Where it came from, if coming back there ends the carry (an edit's timeline). */
  home?: CarryHome;
}

/** Where a carry came from: back over `el`, it ends and `back` takes the press on, at its x. */
export interface CarryHome {
  el: HTMLElement;
  back: (x: number, y: number) => void;
}

/** A carry's ghost: its front card (`card`) and those behind it (`deck`), on `el`. */
interface Ghost {
  el: HTMLElement;
  front: ReturnType<typeof card>;
  behind: ReturnType<typeof card>[];
}

/** What's carried (`carry`, one thing; `carrySet`, several), as the one way of carrying them (`carryWith`) needs it. */
interface Payload {
  /** What an edit takes of it, in order. */
  items: Put[];
  /** Its stretches of the open VOD: a VOD's clip bar keeps them as clips. */
  stretches: { start: number; end: number }[];
  ghost: Ghost;
  /** The ghost as one PNG, for the system's drag. */
  image: () => Promise<Uint8Array | undefined>;
  /** Says what's carried (`clips.carrying`, the stretch; `clips.carried`, how many). */
  lift: () => void;
  /** Puts it on `target`, a place that takes something of it (`takerAt`); true
   *  if the clips `made` for a drag out are kept (a VOD's clip bar shows them). */
  put: (target: Target, made: Promise<Clip[]> | null) => boolean;
  /** Makes the clips whose files go out past the window's edge (not in the
   *  clip bar: only for the drag); with none, the carry ends there. */
  files?: () => Promise<Clip[]>;
  /** Whether those are made as the carry begins (where that's quick: from a file here), not at the edge. */
  early?: boolean;
  /** The VOD they're clips of, if it's one: made in the format clips are made in. */
  vodId?: number;
  /** Where it came from, if coming back there ends the carry. */
  home?: CarryHome;
  /** Where on screen the thing pressed is, for the ghost to fly from, and the pointer. */
  from: DOMRect;
  x: number;
  y: number;
}

/** What of `p` the place under the pointer, `target`, takes into an edit:
 *  on the open edit's timeline, not its own stretches (that would copy them
 *  in place), nor an edit into itself. (A VOD's clip bar takes its stretches.) */
function takes(p: { items: Put[] }, target: Target): Put[] {
  const into = target.edit === "new" ? null : target.timeline ? edits.openId : target.edit;
  const own = (put: Put) => (put.kind === "clip" && put.id === into) || (!!target.timeline && put.kind === "span");
  if (target.edit !== null || target.timeline) return p.items.filter((put) => !own(put));
  return target.bar && clips.doc?.kind === "edit" ? p.items : [];
}

/** What under (`px`, `py`) takes something of `p`; nothing, if nothing would
 *  go there: only that lights up as where it drops. */
function takerAt(p: Payload, px: number, py: number): Target {
  const target = targetAt(px, py);
  const vodBar = target.bar && clips.doc?.kind === "vod";
  return (vodBar ? p.stretches.length : takes(p, target).length) ? target : NOWHERE;
}

/** Brought back over the window, `p`'s system drag goes where a carry of it would. */
function bringsBack(p: Payload) {
  const b: NonNullable<typeof back> = {
    over(x, y) {
      p.lift();
      showTarget(takerAt(p, x, y));
    },
    dropped(x, y) {
      if (p.put(takerAt(p, x, y), null)) b.kept = true;
    },
  };
  return b;
}

/**
 * Carries `p` from (`x`, `y`) while the button is down: see the top of this
 * file. Resolves once it's dropped, taken out of the window, or let go.
 */
function carryWith(p: Payload): Promise<void> {
  const ghost = p.ghost.el;
  const place = showGhost(ghost, p.from, p.x, p.y);
  p.lift();
  document.body.classList.add("timeline-drag");
  // Text the press began selecting (a pill's name) isn't carried along.
  window.getSelection()?.removeAllRanges();

  return new Promise((resolve) => {
    let down = true;
    let leaving = false;
    // Made ahead for the system's drag, where that's quick, rather than once
    // it's out of the window, where nothing can be dropped until it's there.
    let early = p.early && p.files ? p.files() : null;
    const forgetAll = (made: Promise<Clip[]> | null) => made?.then((list) => list.forEach(forgetHidden));
    const finish = () => {
      // Made ahead for a drag out that didn't come: they go.
      forgetAll(early);
      early = null;
      window.removeEventListener("pointermove", onMove, true);
      window.removeEventListener("pointerup", onUp, true);
      window.removeEventListener("pointercancel", onCancel, true);
      window.removeEventListener("keydown", onKey, true);
      clips.carrying = null;
      clips.carried = 0;
      showTarget(NOWHERE);
      document.body.classList.remove("timeline-drag");
      ghost.remove();
      for (const c of [p.ghost.front, ...p.ghost.behind]) c.done();
      resolve();
    };
    // Let go where the page didn't see it (outside the window, or on a
    // question the system asked at the edge): the carry is over, dropped
    // nowhere, and no drag out begins.
    const letGo = () => {
      down = false;
      if (!leaving) finish();
    };
    const onMove = (e: PointerEvent) => {
      if (!holds(e)) return letGo();
      if (leaving) return;
      const box = p.home?.el.getBoundingClientRect();
      if (
        p.home &&
        box &&
        e.clientX >= box.left &&
        e.clientX <= box.right &&
        e.clientY >= box.top &&
        e.clientY <= box.bottom
      ) {
        finish();
        p.home.back(e.clientX, e.clientY);
        return;
      }
      place(e.clientX, e.clientY);
      showTarget(takerAt(p, e.clientX, e.clientY));
      if (atEdge(e)) takeOut();
    };
    const onUp = (e: PointerEvent) => {
      down = false;
      if (leaving) return;
      // A drop on a VOD's clip bar keeps the clips made ahead (the same
      // stretches): waited for, then shown. Anywhere else they go, as in finish.
      const ahead = early;
      early = null;
      finish();
      if (!p.put(takerAt(p, e.clientX, e.clientY), ahead)) forgetAll(ahead);
    };
    const onCancel = () => letGo();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !leaving) {
        e.preventDefault();
        finish();
      }
    };
    // Past the window's edge: the system's drag of the files (in the format
    // clips are made in), if the button's still down: at once, or once each
    // is made where the system can't wait for them (`beforeMade`).
    const takeOut = async () => {
      leaving = true;
      const ahead = early;
      early = null;
      if (!p.files) return finish();
      // From here to "drag-start" is what's waited at the edge (tests time it).
      performance.mark("carry-out");
      clips.overBar = false;
      p.ghost.front.el.classList.add("making");
      let made: Clip[] = [];
      let dragged = false;
      const promised = beforeMade();
      try {
        made = await (ahead ?? p.files());
        const waits = !(await promised);
        const ready: Clip[] = [];
        for (const clip of made) {
          if (!down) break;
          const cut = waits ? await clips.ready(clip.id, () => down, p.vodId) : clip;
          const want = (c: Clip) => (waits ? inFormat(c, p.vodId!, () => down) : remade(c, p.vodId!));
          const done = cut && p.vodId !== undefined ? await want(cut) : cut;
          if (!done) break;
          ready.push(done);
        }
        // The files can be ready as the edge is crossed (a short clip from a
        // file here, on a slow machine); the release then arrives during the
        // waits below, so the button is checked again before the drag is
        // asked for (dragOut), not only here.
        if (made.length && ready.length === made.length && down) {
          const png = await p.image();
          back = bringsBack(p);
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
    window.addEventListener("pointercancel", onCancel, true);
    window.addEventListener("keydown", onKey, true);
  });
}

/**
 * Carries a stretch from (`x`, `y`) while the button is down: see the top of
 * this file. Resolves once it's dropped, taken out of the window, or let go.
 */
export function carry({
  vod,
  start,
  end,
  ownPlayer,
  levels,
  from,
  x,
  y,
  title,
  clip,
  highlight,
  home,
}: Carry): Promise<void> {
  // A stretch of the VOD shown is made a clip through its clip bar; another's (an edit's piece) directly.
  const make = async (shown: boolean): Promise<Clip | null> => {
    if (vod.id === clips.vodId) return exportStretch(start, end, shown);
    return api.addClip(vod.id, start, end, shown).catch((e) => {
      clips.error = String(e);
      return null;
    });
  };
  const label = stretchSaid(start, end);
  const stretch: Stretch = { vodId: vod.id, start, end, title, clip, highlight };
  const front = card(Promise.resolve({ vod, start, end, ownPlayer, levels }), label);
  // The picture for the system's drag, made as soon as its parts are drawn.
  const image = front.image.then((img) => snapshot(img, front.wave, label));
  // A clip's pill goes in as the clip (`edits.add`) and a highlight as a
  // group named for it, both one band; a stretch as a range.
  const into = (editId: number, index?: number) =>
    clip !== undefined
      ? edits.add(editId, clip, index)
      : highlight
        ? edits.put(editId, [putOf(stretch)], false, index)
        : edits.addRange(editId, stretch, index);
  return carryWith({
    items: [clip !== undefined ? { kind: "clip", id: clip } : putOf(stretch)],
    stretches: [{ start, end }],
    ghost: { el: front.el, front, behind: [] },
    image: () => image,
    lift: () => (clips.carrying = { start, end }),
    put({ edit, timeline, bar }, made) {
      if (edit === "new") edits.createWith(stretch);
      else if (edit !== null) into(edit);
      else if (timeline && edits.openId !== null) into(edits.openId, timeline.index);
      else if (bar && clips.doc?.kind === "edit")
        edits.put(clips.doc.id, [{ kind: "range", vodId: vod.id, start, end, title }], true);
      else if (bar) {
        void (made ?? Promise.resolve([])).then(() => make(true));
        return true;
      }
      return false;
    },
    // Its file is cut ahead where that's quick (from a file here: copied, not downloaded).
    files: () => make(false).then((c) => (c ? [c] : [])),
    early: ownPlayer,
    vodId: vod.id,
    home,
    from,
    x,
    y,
  });
}

/** Several things picked (selection.svelte.ts), carried together (`carrySet`). */
export interface CarrySet {
  /** What they are, as an edit takes them, in the order picked. */
  items: Put[];
  /** What the ghost says under its picture: "3 clips" (one alone: as `carry` says it). */
  label: string;
  /** How the first few look (`Face`), in the order picked: the ghost shows
   *  the first's picture, with up to two more behind it. */
  faces?: Face[] | Promise<Face[]>;
  /** The open VOD's stretches among them: its clip bar keeps them as clips. */
  stretches: { start: number; end: number }[];
  /** Makes the clips whose files go out past the window's edge (one only for
   *  the drag isn't in the clip bar); none for rows of the library. */
  files?: () => Promise<Clip[]>;
  /** Whether those are made as the carry begins (where that's quick: from a file here), not at the edge. */
  early?: boolean;
  /** The open VOD, if it's its: the files are made in the format clips are made in. */
  vodId?: number;
  /** Where it came from, if coming back there ends the carry (an edit's timeline). */
  home?: CarryHome;
  /** Where on screen the item pressed is, for the ghost to fly from. */
  from: DOMRect;
  x: number;
  y: number;
}

/** How a thing carried looks in the ghost: stretch `start`..`end` of VOD
 *  `vodId` (to the VOD's end, if no `end`). */
export interface Face {
  vodId: number;
  start: number;
  end?: number;
}

/** How many cards show behind the first in a ghost of several. */
const BEHIND = 2;
/** How many blank cards show behind those, past three things: the pile's thickness, capped where
 *  the cards' edges, SPREAD shared among them, would be under 2 px apart and merge. */
const BLANK = 7;
/** How far, in pixels right and down, the pile's last card sits from the front: the same for any pile. */
const SPREAD = 18;

/** `faces`, the first few, as ghost cards show them; one whose VOD is gone, as none. */
async function looksOf(faces: Face[] | Promise<Face[]>): Promise<(Look | null)[]> {
  const list = (await faces).slice(0, 1 + BEHIND);
  if (!list.length) return [];
  const vods = await api.listVods().catch(() => [] as Vod[]);
  return list.map((f) => {
    const vod = vods.find((v) => v.id === f.vodId);
    return vod ? { vod, start: f.start, end: f.end ?? vod.duration ?? f.start, ownPlayer: playsFromFile(vod) } : null;
  });
}

/**
 * The ghost of `count` things carried together, which look as `faces` says:
 * one alone, as `carry`'s, its card captioned `label`; several, the first's
 * card, captioned `label` ("3 clips"), on top of up to BEHIND more and,
 * past those, up to BLANK blank cards, offset as a deck is.
 */
function deck(faces: Face[] | Promise<Face[]>, count: number, label: string) {
  const looks = looksOf(faces);
  const front = card(
    looks.then((l) => l[0] ?? null),
    label,
  );
  const behind = Array.from({ length: Math.min(BEHIND, count - 1) }, (_, i) =>
    card(looks.then((l) => l[i + 1] ?? null)),
  );
  if (!behind.length) return { el: front.el, front, behind };
  const el = document.createElement("div");
  el.className = "carry-stack";
  // The pile's depth stays the same, SPREAD px right and down, however many
  // cards are in it: the more there are, the closer they stack.
  const stacked = Math.min(BEHIND, count - 1) + Math.max(0, Math.min(BLANK, count - 1 - BEHIND));
  const step = SPREAD / stacked;
  const shift = (el: HTMLElement, i: number) => {
    el.style.transform = `translate(${(i * step).toFixed(1)}px, ${(i * step).toFixed(1)}px)`;
  };
  behind.forEach((c, i) => {
    c.el.classList.add("behind");
    shift(c.el, i + 1);
  });
  // The cards tell one, two and three apart; past three, blank cards behind
  // them (up to BLANK more) make the pile visibly thicker the more there
  // are, the caption under the front card giving the count.
  const blanks = Array.from({ length: Math.min(BLANK, count - 1 - BEHIND) }, (_, j) => {
    const b = document.createElement("div");
    b.className = "carry-ghost behind blank";
    shift(b, BEHIND + 1 + j);
    return b;
  });
  el.append(...blanks.reverse(), ...behind.map((c) => c.el).reverse(), front.el);
  return { el, front, behind };
}

/**
 * Carries several things picked from (`x`, `y`) while the button is down, as
 * `carry` carries one: onto an edit's row (at the end of its timeline), "Make
 * new edit", the open edit's timeline (where the marker shows) or the clip
 * bar (clips there), all in the order picked, as one change. Past the
 * window's edge, the system's drag of all their files, once they're made
 * (rows of the library have none: the carry ends there). Its ghost is
 * `carry`'s for the first, with the next stacked behind it (`deck`), so one
 * looks as it would carried alone. Resolves once it's dropped, taken out of
 * the window, or let go.
 */
export function carrySet(set: CarrySet): Promise<void> {
  const ghost = deck(set.faces ?? [], set.items.length, set.label);
  return carryWith({
    items: set.items,
    stretches: set.stretches,
    ghost,
    image: () => ghost.front.image.then((img) => snapshot(img, ghost.front.wave, set.label)),
    lift: () => (clips.carried = set.items.length),
    put(target, made) {
      const items = takes(set, target);
      if (target.edit === "new") edits.createWith(items);
      else if (target.edit !== null) edits.put(target.edit, items, false);
      else if (target.timeline && edits.openId !== null) edits.put(edits.openId, items, false, target.timeline.index);
      else if (target.bar && clips.doc?.kind === "edit") edits.put(clips.doc.id, items, true);
      else if (target.bar) {
        // On a VOD's clip bar, the clips made for a drag out are the ones
        // kept: those made ahead waited for first, so they're not made twice.
        void (made ?? Promise.resolve([])).then(() => exportStretches(set.stretches));
        return true;
      }
      return false;
    },
    files: set.files,
    early: set.early,
    vodId: set.vodId,
    home: set.home,
    from: set.from,
    x: set.x,
    y: set.y,
  });
}

/**
 * What a press held on a VOD's picture carries: the stretch between cuts
 * that the playhead (`time`, in a VOD `length` long) is in, flying from the
 * timeline (or the picture, `box`); none with no cuts.
 */
export function stretchCarry(vod: Vod, time: number, length: number, box: HTMLElement | undefined) {
  const range = stretchAt(time, length);
  if (!range || !cuts.list.length) return null;
  return (x: number, y: number) => {
    const from = rectOf(range.start, range.end) ?? box?.getBoundingClientRect();
    if (from) carry({ vod, start: range.start, end: range.end, ownPlayer: true, from, x, y });
  };
}

/**
 * A press held still on a player's picture carries what `carryAt()` says
 * (`stretchCarry` on a VOD's; on an edit's, the stretch or piece at the
 * playhead), started at the pointer; a press let go first is a click,
 * which plays or pauses (`toggle`). The picture's pointer handlers; `stop()`
 * when the player goes.
 */
export function holdToCarry(carryAt: () => ((x: number, y: number) => void) | null | undefined, toggle: () => void) {
  let hold: ReturnType<typeof setTimeout> | undefined;
  let held = false;
  let pointer = { x: 0, y: 0 };
  const stop = () => clearTimeout(hold);
  return {
    down(e: PointerEvent) {
      if (e.button !== 0) return;
      held = false;
      pointer = { x: e.clientX, y: e.clientY };
      const start = carryAt();
      if (!start) return;
      hold = setTimeout(() => {
        held = true;
        start(pointer.x, pointer.y);
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
