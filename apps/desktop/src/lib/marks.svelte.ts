// What the transcript view (Transcript.svelte) marks in a VOD's chat: the
// Timeline layers' matches (layers.svelte.ts: saved chat searches) and the
// events turned on (events.ts), drawn on the minimap and highlighted in chat.
// Also the emotes the chat writes as words (7TV, BTTV, FFZ), shown as images
// in chat and in the Timeline panel's pills. Made while the view is set up,
// since it runs effects.

import { api, type ChatOverview, type Vod, type WordEmote } from "./api";
import { EVENTS, eventColor, eventUsd, type EventKind } from "./events";
import { layers } from "./layers.svelte";
import { settings } from "./settings.svelte";

/** What the marks follow, read each time it's needed. */
export interface MarksOf {
  vod: Vod;
  /** Whether the VOD has (or is getting) a chat to search. */
  canHaveChat: boolean;
  /** Messages in the chat so far. */
  chatCount: number;
  /** The chat's counts and events (`chat_overview`). */
  overview: ChatOverview | null;
}

interface Match {
  id: string;
  color: string;
  /** The ids of the messages it matched. */
  ids: Set<number>;
}

// While chat arrives, the layers' searches run at most this often, not on every batch.
const MATCH_EVERY = 5000;

export class Marks {
  #of: () => MarksOf;

  /** The platform's own emotes used in the VOD's chat, by lowercase name. */
  #chatEmotes = $state(new Map<string, string>());
  /** The words the chat's emote providers show as emotes, by exact name. */
  words = $state(new Map<string, WordEmote>());
  /** Each shown layer's matches, and when they were said. */
  layerMatches = $state<(Match & { times: number[] })[]>([]);

  constructor(of: () => MarksOf) {
    this.#of = of;

    $effect(() => {
      const { vod, canHaveChat } = this.#of();
      const id = vod.id;
      void vod.chat;
      this.#chatEmotes = new Map();
      if (!canHaveChat) return;
      api.chatEmotes(id).then(
        (list) =>
          this.#of().vod.id === id && (this.#chatEmotes = new Map(list.map(([n, url]) => [n.toLowerCase(), url]))),
        () => {},
      );
    });

    // From the providers turned on in Settings (none at first), again when
    // they change. Fetched once a day per channel (the app keeps the list),
    // so this is usually quick; until it's in, those words show as words.
    void settings.load();
    // Only the latest lookup's answer counts: toggling quickly sends several,
    // and an older one can finish last.
    let wordsSeq = 0;
    let wordsOf: number | null = null;
    $effect(() => {
      const { vod, canHaveChat } = this.#of();
      const id = vod.id;
      // Another VOD's (another channel's) emotes go at once; the same VOD's
      // stay until the new list is in, rather than flickering to words.
      if (wordsOf !== id) {
        wordsOf = id;
        this.words = new Map();
      }
      const on = (settings.value?.emote_providers ?? []).filter((p) => p.on).map((p) => p.id);
      const mine = ++wordsSeq;
      if (!canHaveChat || (vod.platform !== "twitch" && vod.platform !== "kick") || on.length === 0) {
        this.words = new Map();
        return;
      }
      api.chatWordEmotes(id, on).then(
        (list) => mine === wordsSeq && (this.words = new Map(list.map((e) => [e.name, e]))),
        (e) => mine === wordsSeq && console.warn("word emotes:", e),
      );
    });

    let seq = 0;
    let matchedAt = 0;
    $effect(() => {
      const { vod, chatCount } = this.#of();
      const id = vod.id;
      const wanted = this.#shownLayers.map((l) => ({ id: l.id, color: l.color, terms: l.terms.map((t) => t.text) }));
      const arriving = vod.chat === "pending";
      const mine = ++seq;
      if (wanted.length === 0 || chatCount === 0) {
        this.layerMatches = [];
        return;
      }
      const run = () => {
        matchedAt = Date.now();
        api
          .chatMatches(
            id,
            wanted.map((w) => w.terms),
          )
          .then(
            (found) =>
              mine === seq &&
              (this.layerMatches = wanted.map((w, i) => ({
                id: w.id,
                color: w.color,
                ids: new Set(found[i].map(([id]) => id)),
                times: found[i].map(([, time]) => time),
              }))),
            () => mine === seq && (this.layerMatches = []),
          );
      };
      const wait = arriving ? Math.max(0, MATCH_EVERY - (Date.now() - matchedAt)) : 0;
      if (wait === 0) return void run();
      const timer = setTimeout(run, wait);
      return () => clearTimeout(timer);
    });
  }

  /** Emote images by lowercase name, for the Timeline panel's pills: the
   *  platform's own as used in this chat, then the providers'. */
  emotes = $derived.by(() => {
    const out = new Map(this.#chatEmotes);
    for (const e of this.words.values())
      if (!e.overlay && !out.has(e.name.toLowerCase())) out.set(e.name.toLowerCase(), e.url);
    return out;
  });

  /** Whether the VOD has a chat to mark at all. */
  #active = $derived.by(() => {
    const { vod } = this.#of();
    return vod.platform !== "local" && vod.chat !== "none";
  });
  #shownLayers = $derived.by(() => (this.#active ? layers.shownFor(this.#of().vod) : []));

  /** The chat's events (the overview has them all, few as they are). */
  #events = $derived.by(() => this.#of().overview?.events ?? []);

  /** How many of each event the chat has. */
  eventCounts = $derived.by(() => {
    const out = new Map<EventKind, number>();
    for (const m of this.#events) if (m.kind !== "message") out.set(m.kind, (out.get(m.kind) ?? 0) + 1);
    return out;
  });

  #eventMatches = $derived.by((): Match[] => {
    if (!this.#active) return [];
    const color = eventColor(this.#of().vod.platform);
    return EVENTS.filter((e) => layers.events.includes(e.kind)).map((e) => ({
      id: e.kind,
      color,
      ids: new Set(this.#events.filter((m) => m.kind === e.kind).map((m) => m.id)),
    }));
  });
  #all = $derived([...this.#eventMatches, ...this.layerMatches]);

  /** The layers' matches over the VOD, for the minimap. */
  overlays = $derived(this.layerMatches.map((m) => ({ color: m.color, times: m.times })));

  /** The events turned on, with what each was worth, for the minimap. */
  eventMarks = $derived.by(() => {
    if (this.#eventMatches.length === 0) return [];
    const on = new Set(layers.events);
    const platform = this.#of().vod.platform;
    return this.#events
      .filter((m) => m.kind !== "message" && on.has(m.kind))
      .map((m) => ({ time: m.time, value: eventUsd(m, platform) }));
  });

  /** How many messages each shown layer matched, by layer id. */
  counts = $derived(new Map(this.layerMatches.map((m) => [m.id, m.ids.size])));

  /** The color of the first shown event or layer that matched message `id`, if any. */
  highlightOf(id: number): string | null {
    return this.#all.find((m) => m.ids.has(id))?.color ?? null;
  }
}
