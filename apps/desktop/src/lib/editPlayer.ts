// The player for an edit (EditView.svelte): its pieces one after another, from
// the clips' files, as one stretch of time. One <video> plays them: within a
// clip it jumps over what's struck, and between clips it changes file, which
// keeps the same player (a second <video> would take WebKitGTK over a second
// to start, against a fraction of one to change file). It's an
// EmbeddedPlayer like the others, so the same controls (PlayerBar.svelte) and
// timeline work it.

import type { EmbeddedPlayer } from "./embed";
import { DEFAULT_FPS } from "./media";
import { load, save } from "./stored";

/** A stretch of a clip's file that plays, `from`..`to` seconds into it, at `at` in the edit. */
export interface Segment {
  clipId: number;
  /** Where its clip's file is served from; null until it's made (and known). */
  url: string | null;
  title: string;
  from: number;
  to: number;
  at: number;
}

const VOLUME = "vodscribe.volume";

/** Resolves on the first of `events` on `v`, or after `ms` (so a missing event can't stall it). */
function once(v: HTMLVideoElement, events: string[], ms = 3000): Promise<void> {
  return new Promise((done) => {
    const end = () => {
      clearTimeout(timer);
      for (const e of events) v.removeEventListener(e, end);
      done();
    };
    const timer = setTimeout(end, ms);
    for (const e of events) v.addEventListener(e, end);
  });
}

export class EditPlayer implements EmbeddedPlayer {
  private video: HTMLVideoElement;
  /** The clip whose file is loaded. */
  private clip: number | null = null;
  private segments: Segment[] = [];
  private index = 0;
  private frame = 0;
  private frameRate = DEFAULT_FPS;
  private destroyed = false;
  /** Counts calls to `show`: one superseded by a later one (a quick second seek) stops. */
  private showing = 0;

  constructor(
    host: HTMLElement,
    /** Told why it can't play (a clip not made yet), or null once it can. */
    private onerror: (message: string | null) => void,
  ) {
    const v = document.createElement("video");
    v.playsInline = true;
    v.preload = "auto";
    v.style.cssText = "position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; background: #000";
    host.appendChild(v);
    this.video = v;
    const saved = Number(load(VOLUME));
    if (load(VOLUME) !== null && saved >= 0 && saved <= 1) v.volume = saved;
    v.addEventListener("play", () => this.follow());
    v.addEventListener("ended", () => this.advance());
  }

  /** What plays now. The position stays where it is in the edit, as near as it can. */
  setSegments(segments: Segment[], fps: number | null) {
    const t = this.time();
    const wasPlaying = this.playing();
    this.segments = segments;
    this.frameRate = fps ?? DEFAULT_FPS;
    if (!segments.length) {
      this.video.pause();
      return;
    }
    this.seek(Math.min(t, this.total()), wasPlaying);
  }

  private total(): number {
    const last = this.segments[this.segments.length - 1];
    return last ? last.at + (last.to - last.from) : 0;
  }

  /** Shows segment `i`, `into` seconds in, playing if `play`. */
  private async show(i: number, into: number, play: boolean) {
    const seg = this.segments[i];
    if (!seg) return;
    const mine = ++this.showing;
    const v = this.video;
    if (!seg.url) {
      v.pause();
      this.index = i;
      this.onerror(`“${seg.title}” isn't made yet.`);
      return;
    }
    this.onerror(null);
    if (this.clip !== seg.clipId) {
      this.clip = seg.clipId;
      v.src = seg.url;
    }
    // Its length known before it can be sought in.
    if (v.readyState < HTMLMediaElement.HAVE_METADATA) await once(v, ["loadedmetadata", "error"]);
    if (this.destroyed || mine !== this.showing) return;
    this.index = i;
    const target = seg.from + into;
    if (Math.abs(v.currentTime - target) > 0.001) {
      const sought = once(v, ["seeked", "error"]);
      v.currentTime = target;
      if (!play) await sought;
    }
    if (mine !== this.showing) return;
    if (play) await v.play().catch(() => {});
    else v.pause();
  }

  private advance() {
    if (this.index + 1 < this.segments.length) this.show(this.index + 1, 0, true);
    else this.video.pause();
  }

  /** While playing: on to the next segment as this one ends (over what's struck). */
  private follow() {
    cancelAnimationFrame(this.frame);
    if (this.destroyed || this.video.paused) return;
    const seg = this.segments[this.index];
    if (seg && this.video.currentTime >= seg.to - 0.5 / this.frameRate) this.advance();
    this.frame = requestAnimationFrame(() => this.follow());
  }

  seek(seconds: number, play: boolean) {
    const t = Math.max(0, Math.min(this.total(), seconds));
    let i = 0;
    this.segments.forEach((s, k) => s.at <= t + 1e-6 && (i = k));
    const seg = this.segments[i];
    if (!seg) return;
    this.show(i, Math.max(0, Math.min(t - seg.at, seg.to - seg.from - 0.5 / this.frameRate)), play);
  }

  time(): number {
    const seg = this.segments[this.index];
    if (!seg) return 0;
    return seg.at + Math.max(0, Math.min(seg.to - seg.from, this.video.currentTime - seg.from));
  }

  playing(): boolean {
    return !this.video.paused;
  }

  togglePlay() {
    // Whatever was on its way to showing (paused) gives way.
    this.showing++;
    if (this.playing()) return this.video.pause();
    if (this.time() >= this.total() - 0.05) this.seek(0, true);
    else if (this.clip === null) this.seek(this.time(), true);
    else this.video.play().catch(() => {});
  }

  step(frames: number) {
    this.video.pause();
    this.seek(this.time() + frames / this.frameRate, false);
  }

  duration(): number {
    return this.total();
  }

  fps(): number | null {
    return this.frameRate;
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
    cancelAnimationFrame(this.frame);
    this.video.pause();
    // Stop the download too: a <video> removed while loading keeps fetching.
    this.video.removeAttribute("src");
    this.video.load();
    this.video.remove();
  }
}
