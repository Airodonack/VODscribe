// What the transcript view (Transcript.svelte) marks in a VOD's chat, or an
// edit's (its pieces' VODs' chats where they play): the Timeline layers'
// matches (layers.svelte.ts: saved chat searches) and the events turned on
// (events.ts), drawn on the minimap and highlighted in chat. Also the emotes
// the chat writes as words (7TV, BTTV, FFZ), shown as images in chat and in
// the Timeline panel's pills. Made while the view is set up, since it runs
// effects.

import { api, type ChatOverview, type Vod, type WordEmote } from "./api";
import { EVENTS, eventColor, eventUsd, type EventKind } from "./events";
import { layers } from "./layers.svelte";
import { settings } from "./appSettings.svelte";

/** What the marks follow: getters, so each effect and derived tracks only
 *  what it reads (an overview refreshed while chat downloads mustn't refetch
 *  the emotes, or restart the layers' searches). */
export interface MarksOf {
  /** Whose chat it is: the VOD, or each VOD an edit's pieces are of. */
  vods: Vod[];
  /** Where VOD `vodId`'s moment `time` shows on the timeline shown (an
   *  edit's: where it first plays), if it does. */
  toTimeline: (vodId: number, time: number) => number | null;
  /** Whether the chat has (or is getting) messages to search. */
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
  #of: MarksOf;

  /** The platform's own emotes used in the VOD's chat, by lowercase name. */
  #chatEmotes = $state(new Map<string, string>());
  /** The words the chat's emote providers show as emotes, by exact name. */
  words = $state(new Map<string, WordEmote>());
  /** Each shown layer's matches, and when they were said. */
  layerMatches = $state.raw<(Match & { times: number[] })[]>([]);

  constructor(of: MarksOf) {
    this.#of = of;

    $effect(() => {
      const { vods, canHaveChat } = this.#of;
      const key = vods.map((v) => `${v.id} ${v.chat}`).join();
      this.#chatEmotes = new Map();
      if (!canHaveChat || vods.length === 0) return;
      let stale = false;
      Promise.all(vods.map((v) => api.chatEmotes(v.id).catch(() => []))).then((lists) => {
        if (stale || key !== this.#of.vods.map((v) => `${v.id} ${v.chat}`).join()) return;
        this.#chatEmotes = new Map(lists.flat().map(([n, url]) => [n.toLowerCase(), url]));
      });
      return () => (stale = true);
    });

    // From the providers turned on in Settings (none at first), again when
    // they change. Fetched once a day per channel (the app keeps the list),
    // so this is usually quick; until it's in, those words show as words.
    void settings.load();
    // Only the latest lookup's answer counts: toggling quickly sends several,
    // and an older one can finish last.
    let wordsSeq = 0;
    let wordsOf: string | null = null;
    $effect(() => {
      const { vods, canHaveChat } = this.#of;
      const withWords = vods.filter((v) => v.platform === "twitch" || v.platform === "kick");
      const key = withWords.map((v) => v.id).join();
      // Other VODs' (other channels') emotes go at once; the same VODs'
      // stay until the new list is in, rather than flickering to words.
      if (wordsOf !== key) {
        wordsOf = key;
        this.words = new Map();
      }
      const on = (settings.value?.emote_providers ?? []).filter((p) => p.on).map((p) => p.id);
      const mine = ++wordsSeq;
      if (!canHaveChat || withWords.length === 0 || on.length === 0) {
        this.words = new Map();
        return;
      }
      Promise.all(
        withWords.map((v) =>
          api.chatWordEmotes(v.id, on).catch((e) => {
            console.warn("word emotes:", e);
            return [];
          }),
        ),
      ).then((lists) => mine === wordsSeq && (this.words = new Map(lists.flat().map((e) => [e.name, e]))));
    });

    let seq = 0;
    let matchedAt = 0;
    $effect(() => {
      const { vods, chatCount, toTimeline } = this.#of;
      // Asked again with the counts, which an edit's change asks for again:
      // where its matches play moves with its pieces.
      void this.#of.overview;
      // Each VOD's shown layers (a layer may be one channel's), searched in its chat.
      const asked = vods
        .filter((v) => this.#activeFor(v))
        .map((v) => ({
          vod: v.id,
          layers: layers.shownFor(v).map((l) => ({ id: l.id, color: l.color, terms: l.terms.map((t) => t.text) })),
        }))
        .filter((a) => a.layers.length > 0);
      const arriving = vods.some((v) => v.chat === "pending");
      const mine = ++seq;
      if (asked.length === 0 || chatCount === 0) {
        this.layerMatches = [];
        return;
      }
      const run = () => {
        matchedAt = Date.now();
        Promise.all(
          asked.map((a) =>
            api.chatMatches(
              a.vod,
              a.layers.map((l) => l.terms),
            ),
          ),
        ).then(
          (found) => {
            if (mine !== seq) return;
            // A layer shown for several VODs is one: its matches from each, where they play.
            const merged = new Map<string, Match & { times: number[] }>();
            asked.forEach((a, i) =>
              a.layers.forEach((l, j) => {
                const m = merged.get(l.id) ?? { id: l.id, color: l.color, ids: new Set(), times: [] };
                for (const [id, time] of found[i][j]) {
                  const at = toTimeline(a.vod, time);
                  if (at === null) continue;
                  m.ids.add(id);
                  m.times.push(at);
                }
                merged.set(l.id, m);
              }),
            );
            this.layerMatches = [...merged.values()];
          },
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

  /** Whether `vod` has a chat to mark at all. */
  #activeFor = (vod: Vod) => vod.platform !== "local" && vod.chat !== "none";
  #active = $derived.by(() => this.#of.vods.some(this.#activeFor));
  /** The site of the chat's VOD (an edit's first with chat), for what's named or coloured by it. */
  #platform = $derived.by(() => this.#of.vods.find(this.#activeFor)?.platform ?? "local");

  /** The chat's events (the overview has them all, few as they are). */
  #events = $derived.by(() => this.#of.overview?.events ?? []);

  /** How many of each event the chat has. */
  eventCounts = $derived.by(() => {
    const out = new Map<EventKind, number>();
    for (const m of this.#events) if (m.kind !== "message") out.set(m.kind, (out.get(m.kind) ?? 0) + 1);
    return out;
  });

  #eventMatches = $derived.by((): Match[] => {
    if (!this.#active) return [];
    // Each kind once per site: an edit's events are coloured by their VOD's.
    const platforms = [...new Set(this.#events.map((m) => m.platform ?? this.#platform))];
    if (platforms.length === 0) platforms.push(this.#platform);
    return EVENTS.filter((e) => layers.events.includes(e.kind)).flatMap((e) =>
      platforms.map((platform) => ({
        id: `${e.kind} ${platform}`,
        color: eventColor(platform),
        ids: new Set(
          this.#events.filter((m) => m.kind === e.kind && (m.platform ?? this.#platform) === platform).map((m) => m.id),
        ),
      })),
    );
  });
  #all = $derived([...this.#eventMatches, ...this.layerMatches]);

  /** The layers' matches over the VOD, for the minimap. */
  overlays = $derived(this.layerMatches.map((m) => ({ color: m.color, times: m.times })));

  /** The events turned on, with what each was worth, for the minimap. */
  eventMarks = $derived.by(() => {
    if (this.#eventMatches.length === 0) return [];
    const on = new Set(layers.events);
    return this.#events
      .filter((m) => m.kind !== "message" && on.has(m.kind))
      .map((m) => ({ time: m.time, value: eventUsd(m, m.platform ?? this.#platform) }));
  });

  /** How many messages each shown layer matched, by layer id. */
  counts = $derived(new Map(this.layerMatches.map((m) => [m.id, m.ids.size])));

  /** The color of the first shown event or layer that matched message `id`, if any. */
  highlightOf(id: number): string | null {
    return this.#all.find((m) => m.ids.has(id))?.color ?? null;
  }
}
