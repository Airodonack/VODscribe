// The player for an edit's preview (EditView.svelte): one <video> fed, through
// Media Source Extensions, the edit's preview segments one after another,
// each placed at its time in the edit (crates/core preview.rs makes them, all
// alike: the same size, rate and codecs). So it plays across pieces without a
// gap, and every frame is the one the render will have. Only those near the
// playhead are in the buffer (`BEHIND`, `AHEAD`), so an edit hours long plays
// like a short one. Segments are fetched as they're made (`preview-made`);
// where one isn't yet (or its footage is still downloading), the video waits
// there.
// It's an EmbeddedPlayer like the others, so the same controls work it.

import { api, type PreviewSegment } from "./api";
import type { EmbeddedPlayer } from "./embed";
import { load, save } from "./stored";

const VOLUME = "vodscribe.volume";
/** Preview segments' frame rate (crates/core render.rs `PREVIEW`). */
const FPS = 30;
/** Seconds past the playhead whose segments are added to the buffer. */
const AHEAD = 60;
/** Segments further than this behind the playhead, or `AHEAD` and this past it, leave the buffer. */
const BEHIND = 30;
/** How much wider than a segment the append window is, each side (seconds):
 *  under half a frame, so nothing of the segments either side gets in. */
const WINDOW_SLACK = 0.005;
/** The widest gap in the buffer the playhead is moved over when it stops at one (seconds). */
const GAP = 0.5;

type State = "missing" | "loading" | "in";

export class PreviewPlayer implements EmbeddedPlayer {
  private video: HTMLVideoElement;
  private source = new MediaSource();
  private buffer: SourceBuffer | null = null;
  private plan: PreviewSegment[] = [];
  /** Each segment of the plan: not made yet, being added, or in the buffer. */
  private states: State[] = [];
  /** Counts plans: work for an older one stops. */
  private generation = 0;
  private busy = false;
  /** Asked to add segments while adding others: it goes round again. */
  private again = false;
  private destroyed = false;
  /** Checks, while playing, that the playhead moves (`unstick`). */
  private watch: ReturnType<typeof setInterval>;
  private lastTime = -1;
  /** What went wrong, if the video or its buffer failed: said instead of waiting. */
  private failed: string | null = null;
  /** A seek past the stream's length as it is yet (`seek`). */
  private pendingSeek: { time: number; play: boolean } | null = null;

  /** Whether this webview can play preview segments that way. */
  static supported(codecs: string): boolean {
    return typeof MediaSource !== "undefined" && MediaSource.isTypeSupported(codecs);
  }

  constructor(
    host: HTMLElement,
    private codecs: string,
    /** Told when the video waits for a segment still being made, or null once it doesn't. */
    private onwaiting: (message: string | null) => void,
  ) {
    const v = document.createElement("video");
    v.playsInline = true;
    v.style.cssText = "position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; background: #000";
    host.appendChild(v);
    this.video = v;
    const saved = Number(load(VOLUME));
    if (load(VOLUME) !== null && saved >= 0 && saved <= 1) v.volume = saved;
    v.src = URL.createObjectURL(this.source);
    this.source.addEventListener("sourceopen", () => {
      if (this.buffer) return;
      this.buffer = this.source.addSourceBuffer(this.codecs);
      this.buffer.mode = "segments";
      this.buffer.addEventListener("error", () => this.fail("a segment couldn't be added"));
      this.pump();
    });
    v.addEventListener("waiting", () => this.onwaiting(this.waitingFor()));
    v.addEventListener("playing", () => this.onwaiting(null));
    v.addEventListener("seeked", () => this.onwaiting(this.waitingFor()));
    // As the playhead moves, what's near it comes in and what's far goes.
    v.addEventListener("seeking", () => this.pump());
    v.addEventListener("timeupdate", () => this.pump());
    // An error stops the video for good: said, not left as a silent stop.
    v.addEventListener("error", () => this.fail(v.error?.message || `error ${v.error?.code}`));
    this.watch = setInterval(() => this.unstick(), 250);
  }

  private fail(why: string) {
    console.error("preview", why, this.video.error);
    this.failed = `The preview stopped (${why}). Open the edit again to restart it.`;
    this.onwaiting(this.failed);
  }

  /** Moves the playhead over a small gap in the buffer it has stopped at
   *  (between two segments, say, where a webview won't play across one). */
  private unstick() {
    const v = this.video;
    const t = v.currentTime;
    const stuck = !v.paused && !v.seeking && t === this.lastTime;
    this.lastTime = t;
    if (!stuck) return;
    for (let i = 0; i < v.buffered.length; i++) {
      const start = v.buffered.start(i);
      if (start > t && start - t <= GAP) {
        console.warn(`preview: over a gap at ${t.toFixed(3)} to ${start.toFixed(3)}`);
        v.currentTime = start + 0.001;
        return;
      }
    }
  }

  /** The segments to play now. Those in the buffer that are the same (key,
   *  time and length) stay in it; the rest leave it. */
  setPlan(plan: PreviewSegment[]) {
    const id = (s: PreviewSegment) => `${s.key}@${s.at}+${s.length}`;
    const old = new Map(this.plan.map((s, i) => [id(s), this.states[i]]));
    const states: State[] = plan.map((s) => (s.key && old.get(id(s)) === "in" ? "in" : "missing"));
    const kept = new Set(plan.filter((_, i) => states[i] === "in").map(id));
    const gone = this.plan.filter((s, i) => this.states[i] !== "missing" && !kept.has(id(s)));
    if (!gone.length && plan.length === this.plan.length && plan.every((s, i) => id(s) === id(this.plan[i]))) return;
    this.plan = plan;
    this.states = states;
    this.generation++;
    this.restart(gone);
  }

  /** Segment `key` is made: added, if the plan has it. */
  made(key: string) {
    if (this.plan.some((s) => s.key === key)) this.pump();
  }

  private total(): number {
    const last = this.plan[this.plan.length - 1];
    return last ? last.at + last.length : 0;
  }

  /** What the segment at the playhead waits for, if it isn't in the buffer yet. */
  private waitingFor(): string | null {
    if (this.failed) return this.failed;
    const t = this.video.currentTime;
    const i = this.plan.findIndex((s) => t >= s.at && t < s.at + s.length);
    if (i < 0 || this.states[i] === "in") return null;
    return this.plan[i].key ? "Making the preview here…" : "Downloading this part of the video…";
  }

  /** Waits for the buffer to finish what it's doing. */
  private settled(): Promise<void> {
    const b = this.buffer;
    if (!b?.updating) return Promise.resolve();
    return new Promise((done) => b.addEventListener("updateend", () => done(), { once: true }));
  }

  /** Takes segments `gone` out of the buffer, then adds what's missing near the playhead. */
  private async restart(gone: PreviewSegment[]) {
    const mine = this.generation;
    const b = this.buffer;
    if (!b) return;
    while (this.busy) await new Promise((r) => setTimeout(r, 10));
    if (mine !== this.generation || this.destroyed) return;
    this.busy = true;
    try {
      for (const s of gone) await this.removeRange(s.at, s.at + s.length);
    } finally {
      this.busy = false;
    }
    this.pump();
  }

  /** Takes `from`..`to` out of the buffer. */
  private async removeRange(from: number, to: number) {
    const b = this.buffer;
    if (!b) return;
    await this.settled();
    b.remove(from, to);
    await this.settled();
  }

  /** Adds, in order, each segment near the playhead not in yet whose file is
   *  made, after taking out those far from it. */
  private async pump() {
    const b = this.buffer;
    if (!b || this.destroyed) return;
    if (this.busy) {
      this.again = true;
      return;
    }
    this.busy = true;
    this.again = false;
    const mine = this.generation;
    try {
      // As long as the edit, so a seek anywhere in it is kept.
      if (this.source.readyState === "open" && !(Math.abs(this.source.duration - this.total()) < 1e-3)) {
        await this.settled();
        try {
          this.source.duration = this.total();
        } catch (e) {
          // Shorter than what's in still (it's taken out as the plan changes): next time.
          console.warn("preview duration", e);
        }
      }
      this.seekWaiting();
      const t = this.video.currentTime;
      for (let i = 0; i < this.plan.length; i++) {
        const seg = this.plan[i];
        const far = seg.at + seg.length < t - BEHIND || seg.at > t + AHEAD + BEHIND;
        if (this.states[i] === "in" && far) {
          await this.removeRange(seg.at, seg.at + seg.length);
          if (mine !== this.generation) return;
          this.states[i] = "missing";
        }
      }
      for (let i = 0; i < this.plan.length; i++) {
        if (mine !== this.generation || this.destroyed) return;
        const seg = this.plan[i];
        if (this.states[i] === "in" || !seg.key) continue;
        if (seg.at + seg.length < t - 1 || seg.at > t + AHEAD) continue;
        this.states[i] = "loading";
        const bytes = await api.previewSegment(seg.key).catch(() => new Uint8Array());
        if (mine !== this.generation) return;
        if (!bytes.length) {
          this.states[i] = "missing";
          continue;
        }
        await this.settled();
        // Placed at its time, cut to its length (its sound runs a little over).
        // The window opens a little early and closes a little late: its first
        // and last frames are right on its ends, and a frame the webview's
        // rounding put just outside would be dropped, and with the first, all
        // up to the next keyframe (WebKitGTK 2.54 lost each segment's first 2 s).
        b.timestampOffset = seg.at;
        b.appendWindowStart = 0;
        b.appendWindowEnd = seg.at + seg.length + WINDOW_SLACK;
        b.appendWindowStart = Math.max(0, seg.at - WINDOW_SLACK);
        b.appendBuffer(bytes as BufferSource);
        await this.settled();
        this.states[i] = "in";
      }
      // The end, once the last segment is in (ended with nothing in it, WebKit's
      // player never plays); adding to the buffer after opens it again.
      const last = this.states.length > 0 && this.states[this.states.length - 1] === "in";
      if (last && this.source.readyState === "open" && !b.updating) {
        this.source.endOfStream();
      }
    } catch (e) {
      this.fail(String(e));
    } finally {
      this.busy = false;
      this.onwaiting(this.waitingFor());
      if (this.again) this.pump();
    }
  }

  seek(seconds: number, play: boolean) {
    const time = Math.max(0, Math.min(this.total(), seconds));
    // The video can't go past its stream's length, which may still be the
    // plan before's (a shorter edit's): then it goes once that's set (pump).
    if (!(this.video.duration >= time - 1e-3)) {
      this.pendingSeek = { time, play };
      this.pump();
      return;
    }
    this.pendingSeek = null;
    this.video.currentTime = time;
    if (play) this.video.play().catch(() => {});
  }

  /** A seek waiting for the stream's length to reach it, made now if it does. */
  private seekWaiting() {
    const s = this.pendingSeek;
    if (!s || !(this.video.duration >= s.time - 1e-3)) return;
    this.pendingSeek = null;
    this.video.currentTime = s.time;
    if (s.play) this.video.play().catch(() => {});
  }

  time(): number {
    return this.pendingSeek?.time ?? this.video.currentTime;
  }

  playing(): boolean {
    return !this.video.paused;
  }

  togglePlay() {
    if (this.playing()) return this.video.pause();
    if (this.time() >= this.total() - 0.05) this.video.currentTime = 0;
    this.video.play().catch(() => {});
  }

  step(frames: number) {
    this.video.pause();
    this.seek(this.time() + frames / FPS, false);
  }

  duration(): number {
    return this.total();
  }

  fps(): number | null {
    return FPS;
  }

  volume = {
    get: () => this.video.volume,
    muted: () => this.video.muted,
    set: (volume: number) => {
      this.video.volume = volume;
      this.video.muted = volume === 0;
      save(VOLUME, String(volume));
    },
    setMuted: (muted: boolean) => {
      this.video.muted = muted;
    },
  };

  rate = {
    get: () => this.video.playbackRate,
    set: (rate: number) => {
      this.video.playbackRate = rate;
    },
  };

  destroy() {
    this.destroyed = true;
    clearInterval(this.watch);
    this.video.pause();
    URL.revokeObjectURL(this.video.src);
    this.video.removeAttribute("src");
    this.video.load();
    this.video.remove();
  }
}
