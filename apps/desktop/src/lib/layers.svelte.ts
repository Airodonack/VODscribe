// Timeline layers (crates/core `layers`): loaded once, saved shortly after
// each change, and limited to a few drawn at once so the timeline stays legible.

import { api, type Layer, type Vod } from "./api";

/** Layers drawn at once, per VOD. */
export const MAX_VISIBLE = 3;

/** Colors new layers take, in order, skipping ones in use. */
const PALETTE = ["#f59e0b", "#ec4899", "#14b8a6", "#0ea5e9", "#84cc16", "#ef4444", "#a855f7"];

class Layers {
  list = $state<Layer[]>([]);
  #loading: Promise<void> | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  error = $state<string | null>(null);

  load(): Promise<void> {
    this.#loading ??= api.getLayers().then(
      (l) => void (this.list = l),
      (e) => {
        this.#loading = null;
        this.error = String(e);
      },
    );
    return this.#loading;
  }

  /** Saves a moment after the last change, so typing doesn't write on every key. */
  save() {
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => {
      api.setLayers($state.snapshot(this.list)).then(
        () => (this.error = null),
        (e) => (this.error = String(e)),
      );
    }, 400);
  }

  /** Whether `layer` is for `vod`'s channel (or every channel). */
  appliesTo(layer: Layer, vod: Vod): boolean {
    return !layer.channel || (layer.channel.platform === vod.platform && layer.channel.channel === vod.channel);
  }

  forVod(vod: Vod): Layer[] {
    return this.list.filter((l) => this.appliesTo(l, vod));
  }

  /** The layers drawn for `vod`: the visible ones, at most MAX_VISIBLE. */
  shownFor(vod: Vod): Layer[] {
    return this.forVod(vod)
      .filter((l) => l.visible)
      .slice(0, MAX_VISIBLE);
  }

  canShowMore(vod: Vod): boolean {
    return this.shownFor(vod).length < MAX_VISIBLE;
  }

  add(vod: Vod): Layer {
    const used = new Set(this.list.map((l) => l.color));
    const layer: Layer = {
      id: crypto.randomUUID(),
      name: "New layer",
      color: PALETTE.find((c) => !used.has(c)) ?? PALETTE[this.list.length % PALETTE.length],
      terms: [],
      visible: this.canShowMore(vod),
      channel: null,
    };
    this.list.push(layer);
    this.save();
    return this.list[this.list.length - 1];
  }

  remove(id: string) {
    this.list = this.list.filter((l) => l.id !== id);
    this.save();
  }
}

export const layers = new Layers();
