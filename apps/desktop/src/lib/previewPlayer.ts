// The player for an edit's preview (EditView.svelte): one <video> fed, through
// Media Source Extensions, the edit's preview segments one after another,
// each placed at its time in the edit (crates/core preview.rs makes them, all
// alike: the same size, rate and codecs). So it plays across pieces without a
// gap, and every frame is the one the render will have. Segments are fetched
// as they're made (`preview-made`); where one isn't yet (or its footage is
// still downloading), the video waits there.
// It's an EmbeddedPlayer like the others, so the same controls work it.

import { api, type PreviewSegment } from "./api";
import type { EmbeddedPlayer } from "./embed";
import { load, save } from "./stored";

const VOLUME = "vodscribe.volume";
/** Preview segments' frame rate (crates/core render.rs `PREVIEW`). */
const FPS = 30;

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
      this.pump();
    });
    v.addEventListener("waiting", () => this.onwaiting(this.waitingFor()));
    v.addEventListener("playing", () => this.onwaiting(null));
    v.addEventListener("seeked", () => this.onwaiting(this.waitingFor()));
  }

  /** The segments to play now. Unchanged ones are kept; otherwise the buffer starts over. */
  setPlan(plan: PreviewSegment[]) {
    const same =
      plan.length === this.plan.length &&
      plan.every((s, i) => s.key === this.plan[i].key && s.at === this.plan[i].at && s.length === this.plan[i].length);
    if (same) return;
    this.plan = plan;
    this.states = plan.map(() => "missing");
    this.generation++;
    this.restart();
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

  /** Empties the buffer and adds the plan's segments again, from the start. */
  private async restart() {
    const mine = this.generation;
    const b = this.buffer;
    if (!b) return;
    while (this.busy) await new Promise((r) => setTimeout(r, 10));
    if (mine !== this.generation || this.destroyed) return;
    this.busy = true;
    try {
      await this.settled();
      if (b.buffered.length) {
        b.remove(0, Infinity);
        await this.settled();
      }
    } finally {
      this.busy = false;
    }
    this.pump();
  }

  /** Adds, in order, each segment not in yet whose file is made. */
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
      for (let i = 0; i < this.plan.length; i++) {
        if (mine !== this.generation || this.destroyed) return;
        const seg = this.plan[i];
        if (this.states[i] === "in" || !seg.key) continue;
        this.states[i] = "loading";
        const bytes = await api.previewSegment(seg.key).catch(() => new Uint8Array());
        if (mine !== this.generation) return;
        if (!bytes.length) {
          this.states[i] = "missing";
          continue;
        }
        await this.settled();
        // Placed at its time, cut to its length (its sound runs a little over).
        b.timestampOffset = seg.at;
        b.appendWindowStart = 0;
        b.appendWindowEnd = seg.at + seg.length;
        b.appendWindowStart = seg.at;
        b.appendBuffer(bytes as BufferSource);
        await this.settled();
        this.states[i] = "in";
      }
      // The end, once there's a stream to end (ended with nothing in it, WebKit's player never plays).
      const all = this.plan.length > 0 && this.states.every((s) => s === "in");
      if (all && this.source.readyState === "open" && !b.updating) {
        this.source.endOfStream();
      }
    } catch (e) {
      console.error("preview", e);
    } finally {
      this.busy = false;
      this.onwaiting(this.waitingFor());
      if (this.again) this.pump();
    }
  }

  seek(seconds: number, play: boolean) {
    this.video.currentTime = Math.max(0, Math.min(this.total(), seconds));
    if (play) this.video.play().catch(() => {});
  }

  time(): number {
    return this.video.currentTime;
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
    this.video.pause();
    URL.revokeObjectURL(this.video.src);
    this.video.removeAttribute("src");
    this.video.load();
    this.video.remove();
  }
}
