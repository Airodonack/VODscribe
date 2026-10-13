// Timeline layers and event toggles (crates/core `layers`): loaded once, saved
// shortly after each change. A few layers are drawn at once, so the timeline
// stays legible; the events are drawn apart from them (events.ts), any of them.

import { api, type Layer, type Vod } from "./api";
import type { EventKind } from "./events";

/** Term layers drawn at once, per VOD. */
export const MAX_VISIBLE = 3;

/** Colors new layers take, in order, skipping ones in use. */
const PALETTE = ["#f59e0b", "#ec4899", "#ef4444", "#14b8a6", "#0ea5e9", "#84cc16", "#a855f7"];

const EVENT_STYLE = "vodscribe.eventStyle";

class Layers {
  list = $state<Layer[]>([]);
  /** How events are drawn (Minimap.svelte); remembered in this webview only. */
  eventStyle = $state<"bars" | "dots">(readEventStyle());

  setEventStyle(style: "bars" | "dots") {
    this.eventStyle = style;
    try {
      localStorage.setItem(EVENT_STYLE, style);
    } catch {
      // Not remembered, which is fine.
    }
  }

  /** The chat events drawn on the timeline. */
  events = $state<EventKind[]>([]);
  #loading: Promise<void> | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  error = $state<string | null>(null);

  load(): Promise<void> {
    this.#loading ??= api.getTimeline().then(
      (t) => {
        this.list = t.layers;
        this.events = t.events;
      },
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
      api.setTimeline({ layers: $state.snapshot(this.list), events: $state.snapshot(this.events) }).then(
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

  toggleEvent(kind: EventKind) {
    if (this.events.includes(kind)) this.events = this.events.filter((k) => k !== kind);
    else this.events.push(kind);
    this.save();
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

function readEventStyle(): "bars" | "dots" {
  try {
    return localStorage.getItem(EVENT_STYLE) === "dots" ? "dots" : "bars";
  } catch {
    return "bars";
  }
}

export const layers = new Layers();
