// An edit's player in the edit view (EditView.svelte): the app's own
// player (player.rs, crates/player) playing the edit's pieces end to end
// from their footage, drawn on the screen under the view's frame
// (Screen.svelte). Its time runs on the page's clock between what the
// player tells (`player-time`). Where the window has no screen yet, the
// view uses the Media Source preview (previewPlayer.ts) instead; both are an
// `EditPlayback`.

import { mount, unmount } from "svelte";
import { api, events, type PlayerStatus, type PlayerStretch, type UnlistenFn } from "./api";
import { Clock } from "./embed";
import Screen from "./Screen.svelte";

/** What the edit view drives: the preview, or the app's own player. */
export interface EditPlayback {
  /** Plays edit `id` (opening it if it's another), planned again from its pieces and footage as they are now:
   *  its stretches, and which have their footage. */
  plan(id: number, from: number): Promise<PlayerStretch[]>;
  time(): number;
  playing(): boolean;
  togglePlay(): void;
  seek(seconds: number, play: boolean): void;
  step?(frames: number): void;
  fps(): number | null;
  volume: { get(): number; muted(): boolean; set(volume: number): void; setMuted(muted: boolean): void };
  rate: { get(): number; set(rate: number): void };
  /** Made quickly at a lower quality than the render (the preview). */
  lowQuality: boolean;
  destroy(): void;
}

export class EditPlayer implements EditPlayback {
  readonly lowQuality = false;
  private screen: ReturnType<typeof mount>;
  // Runs once the player's time does (`running`): after play it stands
  // until the sound starts.
  private clock = new Clock(0, false);
  private isPlaying = false;
  private editId: number | null = null;
  private opening: Promise<unknown> | null = null;
  private rateAtStart: number | null = null;
  private length = 0;
  private loud = 1;
  private quiet = false;
  private unlisten: UnlistenFn | null = null;
  private gone = false;

  constructor(
    host: HTMLElement,
    private onerror: (message: string | null) => void,
  ) {
    this.screen = mount(Screen, { target: host });
    events.playerTime((s) => !this.gone && this.tell(s)).then((u) => (this.gone ? u() : (this.unlisten = u)));
  }

  private tell(status: PlayerStatus) {
    this.clock.set(status.time);
    this.clock.playing = status.running;
    this.isPlaying = status.playing;
  }

  async plan(id: number, from: number): Promise<PlayerStretch[]> {
    if (this.editId !== id) {
      this.editId = id;
      this.tell({ time: from, playing: false, running: false });
      this.opening = api.playerOpenEdit(id, from, false).then(
        (opened) => {
          if (this.gone || this.editId !== id) return;
          this.rateAtStart = opened.fps;
          this.length = opened.duration;
          api.playerSetVolume(this.quiet ? 0 : this.loud).catch(() => {});
          this.onerror(null);
        },
        (e) => !this.gone && this.onerror(String(e)),
      );
    }
    await this.opening;
    const stretches = await api.playerPlan(id);
    this.length = stretches.reduce((end, s) => Math.max(end, s.at + s.length), 0);
    return stretches;
  }

  time() {
    const t = this.clock.now();
    return this.length ? Math.min(t, this.length) : t;
  }

  playing() {
    return this.isPlaying;
  }

  togglePlay() {
    const playing = !this.isPlaying;
    this.tell({ time: this.time(), playing, running: false });
    (playing ? api.playerPlay() : api.playerPause()).then(
      (s) => this.tell(s),
      () => {},
    );
  }

  seek(seconds: number, play: boolean) {
    this.tell({ time: seconds, playing: play, running: false });
    api.playerSeek(seconds, play).then(
      (s) => this.tell(s),
      () => {},
    );
  }

  step(frames: number) {
    api.playerStep(frames).then(
      (s) => this.tell(s),
      () => {},
    );
  }

  fps() {
    return this.rateAtStart;
  }

  volume = {
    get: () => this.loud,
    muted: () => this.quiet,
    set: (volume: number) => {
      this.loud = volume;
      this.quiet = volume === 0;
      api.playerSetVolume(volume).catch(() => {});
    },
    setMuted: (muted: boolean) => {
      this.quiet = muted;
      api.playerSetVolume(muted ? 0 : this.loud).catch(() => {});
    },
  };

  // The app's player has no speed yet.
  rate = {
    get: () => 1,
    set: () => {},
  };

  destroy() {
    this.gone = true;
    this.unlisten?.();
    unmount(this.screen);
    api.playerClose().catch(() => {});
  }
}
