// An edit's player in the edit view (EditView.svelte): the app's own
// player (player.rs, crates/player) playing the edit's pieces end to end
// from their footage, drawn on the screen under the view's frame
// (Screen.svelte), driven as a file's is (nativeDriver.ts).

import { mount, unmount } from "svelte";
import { api, type PlayerStretch } from "./api";
import { NativeDriver } from "./nativeDriver";
import { prefs } from "./playerPrefs.svelte";
import Screen from "./Screen.svelte";

/** What the edit view drives. */
export interface EditPlayback {
  /** Plays edit `id` (opening it if it's another), planned again from its pieces and footage as they are now:
   *  its stretches, and which have their footage. */
  plan(id: number, from: number): Promise<PlayerStretch[]>;
  time(): number;
  /** Where the frame on screen is, where the player knows (null: none shows). */
  shown?(): number | null;
  playing(): boolean;
  togglePlay(): void;
  seek(seconds: number, play: boolean): void;
  step?(frames: number): void;
  fps(): number | null;
  volume: { get(): number; muted(): boolean; set(volume: number): void; setMuted(muted: boolean): void };
  /** None where the player has no speed (the bar then hides its control). */
  rate?: { get(): number; set(rate: number): void };
  destroy(): void;
}

export class EditPlayer implements EditPlayback {
  private screen: ReturnType<typeof mount>;
  private driver = new NativeDriver();
  private editId: number | null = null;
  private openingEdit: Promise<unknown> | null = null;
  private gone = false;
  /** Plans asked for, so an older one's answer (the player skips it) doesn't set the length. */
  private plans = 0;

  constructor(
    host: HTMLElement,
    private onerror: (message: string | null) => void,
  ) {
    this.screen = mount(Screen, { target: host });
    const saved = prefs.volume();
    if (saved !== null) this.driver.volume.set(saved);
    const rate = prefs.speed();
    if (rate !== null) this.driver.speed.set(rate);
  }

  async plan(id: number, from: number): Promise<PlayerStretch[]> {
    if (this.editId !== id) {
      this.editId = id;
      this.driver.opening(from);
      this.openingEdit = api.playerOpenEdit(id, from, false).then(
        (opened) => {
          if (this.gone || this.editId !== id) return;
          this.driver.opened(opened);
          this.onerror(null);
        },
        (e) => {
          if (this.gone || this.editId !== id) return;
          // Opened again at the next plan, rather than never.
          this.editId = null;
          this.onerror(String(e));
        },
      );
    }
    await this.openingEdit;
    // It didn't open (said by `onerror`): nothing to plan, till it's tried again.
    if (this.editId !== id) return [];
    const mine = ++this.plans;
    const stretches = await api.playerPlan(id);
    if (mine === this.plans) this.driver.length = stretches.reduce((end, s) => Math.max(end, s.at + s.length), 0);
    return stretches;
  }

  time = () => this.driver.time();
  playing = () => this.driver.playing();
  shown = () => this.driver.shown();
  settled = () => this.driver.settled();
  togglePlay = () => this.driver.togglePlay();
  seek = (seconds: number, play: boolean) => this.driver.seek(seconds, play);
  step = (frames: number) => this.driver.step(frames);
  fps = () => this.driver.fps;
  rate = {
    get: () => this.driver.speed.get(),
    set: (rate: number) => {
      this.driver.speed.set(rate);
      prefs.setSpeed(rate);
    },
  };
  volume = {
    get: () => this.driver.volume.get(),
    muted: () => this.driver.volume.muted(),
    set: (volume: number) => {
      this.driver.volume.set(volume);
      prefs.setVolume(volume);
    },
    setMuted: (muted: boolean) => this.driver.volume.setMuted(muted),
  };

  destroy() {
    this.gone = true;
    unmount(this.screen);
    this.driver.destroy();
  }
}
