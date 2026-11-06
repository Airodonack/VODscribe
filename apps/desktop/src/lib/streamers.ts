// Streamer cards: what VODscribe knows of a person who streams, one JSON file
// per person on the file host (crates/core annotations.rs `streamer_card`),
// at `<file host>/streamers/<id>.json`, the id one VODscribe gives them (not a
// platform's: one card whatever sites they're on). A VOD's annotations name
// the card of each person in its cast (`CastMember.streamer`), and the VOD's
// own channel is found by its account in a small index beside them
// (`<file host>/streamers.json`, `cardIdOf`). Everything in a card but its id
// may be missing, and the card (StreamerCard.svelte) shows only what's there.
// The types below are the format, for whatever publishes the files; the
// readers take what they can and leave out the rest, never refusing a whole
// card for one bad field. Pure, so Node tests it (streamers.test.mjs).

import { cardId, readAccount, type Account, type CastMember } from "./annotations";

/** A card's `format` field; `version` counts breaking changes to its shape. */
export const STREAMER_FORMAT = "vodscribe-streamer";
export const STREAMER_VERSION = 1;

/** The index's `format` field, and the version of its shape this reads. */
export const INDEX_FORMAT = "vodscribe-streamers";
export const INDEX_VERSION = 1;

/**
 * One person's card, as published at `streamers/<id>.json`:
 *
 * ```json
 * {
 *   "format": "vodscribe-streamer",
 *   "version": 1,
 *   "id": "marlowe",
 *   "name": "Marlowe",
 *   "avatar": { "platform": "twitch", "login": "marlowe_live" },
 *   "channels": [{ "platform": "twitch", "login": "marlowe_live" }, { "platform": "youtube", "login": "@marlowe" }],
 *   "aliases": ["Mar"],
 *   "socials": [{ "label": "Bluesky", "url": "https://bsky.app/profile/marlowe.example" }],
 *   "contexts": {
 *     "nopixel": {
 *       "characters": [{ "name": "Vance Marlowe", "wiki": "https://nopixel.fandom.com/wiki/Vance_Marlowe" }]
 *     }
 *   }
 * }
 * ```
 */
export type StreamerFile = {
  format: typeof STREAMER_FORMAT;
  version: typeof STREAMER_VERSION;
  /** The id VODscribe gives the person: `cardId`'s shape, the file's own name. */
  id: string;
  /** What they're called, as the card's heading says it. */
  name?: string;
  /** Whose picture the card shows: one of their accounts, asked of the image server (accounts.ts). Never a picture's address. */
  avatar?: Account;
  /** Their channel on each site, as a cast member's `account` is (annotations.ts `readAccount`). */
  channels?: Account[];
  /** Other names they go by. */
  aliases?: string[];
  /** Their other pages; `https` only. Left empty for most: filled where they have a VODscription. */
  socials?: Social[];
  /**
   * What they are in a context, by the context's id (`cardId`'s shape, as a
   * VOD's annotations name it in `contexts`). A section shows only in a VOD
   * whose annotations name its context.
   */
  contexts?: Record<string, ContextSection>;
};

/** A page of theirs elsewhere: what it's called (the site's name, else its address shows) and where. */
export type Social = { label?: string; url: string };

/** A character they play in a context: its name, and its wiki page (`https`; for NoPixel, the NoPixel wiki). */
export type Character = { name: string; wiki?: string };

/** What a card says of a context: its heading (else `CONTEXT_TITLES`', else its id) and the characters they play. */
export type ContextSection = { title?: string; characters?: Character[] };

/**
 * The index, at `streamers.json`: each account's card id, by
 * `"<platform>/<login in lower case>"`.
 *
 * ```json
 * { "format": "vodscribe-streamers", "version": 1, "accounts": { "twitch/marlowe_live": "marlowe" } }
 * ```
 */
export type StreamerIndex = {
  format: typeof INDEX_FORMAT;
  version: typeof INDEX_VERSION;
  accounts: Record<string, string>;
};

/** Contexts' headings, by id, for a section that gives none. */
export const CONTEXT_TITLES: Record<string, string> = { nopixel: "NoPixel" };

/** A card as read: what's there and can be shown, the rest left out. */
export type Streamer = {
  id: string;
  name?: string;
  avatar?: Account;
  channels: Account[];
  aliases: string[];
  socials: Social[];
  contexts: Record<string, { title: string; characters: Character[] }>;
};

/** The most a card brings of each, short of what would make it a page. */
const MOST = 50;

const text = (value: unknown): string | null => {
  if (typeof value !== "string") return null;
  const s = value.trim();
  return s && s.length <= 200 ? s : null;
};

/** `value` as an address a card may link to: `https`, and nothing that isn't one. */
export function httpsUrl(value: unknown): string | null {
  if (typeof value !== "string") return null;
  try {
    const url = new URL(value.trim());
    return url.protocol === "https:" && url.hostname ? url.href : null;
  } catch {
    return null;
  }
}

const list = (value: unknown): unknown[] => (Array.isArray(value) ? value.slice(0, MOST) : []);

/** `account` as one key of the index: "twitch/marlowe_live". */
export const accountKey = (account: Account): string => `${account.platform}/${account.login.toLowerCase()}`;

/**
 * `value` as card `id`: null when it isn't one (not a card, another format
 * or version, another person's id). What's in it that can't be read is left
 * out: a channel on a site not listed, a link that isn't `https`, a section
 * with no characters.
 */
export function readStreamer(value: unknown, id: string): Streamer | null {
  if (typeof value !== "object" || value === null) return null;
  const v = value as Record<string, unknown>;
  if (v.format !== STREAMER_FORMAT || v.version !== STREAMER_VERSION || cardId(v.id) !== id) return null;
  const channels: Account[] = [];
  for (const c of list(v.channels)) {
    const account = readAccount(c);
    if (account && !channels.some((o) => accountKey(o) === accountKey(account))) channels.push(account);
  }
  const aliases = [
    ...new Set(
      list(v.aliases)
        .map(text)
        .filter((a): a is string => a !== null),
    ),
  ];
  const socials: Social[] = [];
  for (const s of list(v.socials)) {
    const url = httpsUrl((s as Record<string, unknown> | null)?.url);
    const label = text((s as Record<string, unknown> | null)?.label);
    if (url) socials.push(label ? { label, url } : { url });
  }
  const contexts: Streamer["contexts"] = {};
  if (typeof v.contexts === "object" && v.contexts !== null && !Array.isArray(v.contexts))
    for (const [key, section] of Object.entries(v.contexts as Record<string, unknown>).slice(0, MOST)) {
      const context = cardId(key);
      if (!context || typeof section !== "object" || section === null) continue;
      const s = section as Record<string, unknown>;
      const characters: Character[] = [];
      for (const c of list(s.characters)) {
        const name = text((c as Record<string, unknown> | null)?.name);
        const wiki = httpsUrl((c as Record<string, unknown> | null)?.wiki);
        if (name) characters.push(wiki ? { name, wiki } : { name });
      }
      if (characters.length)
        contexts[context] = { title: text(s.title) ?? CONTEXT_TITLES[context] ?? context, characters };
    }
  const name = text(v.name);
  const avatar = readAccount(v.avatar);
  return {
    id,
    ...(name ? { name } : {}),
    ...(avatar ? { avatar } : {}),
    channels,
    aliases,
    socials,
    contexts,
  };
}

/**
 * The card id of `account` in `index` (as `streamer_index` gives it, unread),
 * or null: none there, or no index. The one place that knows the index's
 * shape, so it can change here alone.
 */
export function cardIdOf(index: unknown, account: Account): string | null {
  if (typeof index !== "object" || index === null) return null;
  const v = index as Record<string, unknown>;
  if (v.format !== INDEX_FORMAT || v.version !== INDEX_VERSION) return null;
  const accounts = v.accounts;
  if (typeof accounts !== "object" || accounts === null) return null;
  const key = accountKey(account);
  return Object.prototype.hasOwnProperty.call(accounts, key)
    ? cardId((accounts as Record<string, unknown>)[key])
    : null;
}

/** A link on a card: what it says, where it goes, and its tooltip. */
export type CardLink = { label: string; href: string; title?: string };

/** What a card shows: each list empty when there's nothing for it, and then not drawn at all. */
export type CardView = {
  name: string;
  /** Whose picture it shows, if anyone's. */
  avatar: Account | null;
  aliases: string[];
  channels: Account[];
  socials: CardLink[];
  sections: { id: string; title: string; characters: Character[] }[];
};

/** A social link's label: the one given, else the address without its scheme and trailing slash. */
const socialLabel = (s: Social) => s.label ?? s.url.replace(/^https:\/\/(www\.)?/, "").replace(/\/$/, "");

/**
 * What the card of `member` shows in a VOD whose contexts are `contexts`:
 * their card's (`streamer`, null when there's none or it couldn't be had)
 * where it has it, else what the VOD's cast says (`named`: the name the
 * views call them by). Their account in the cast is one of their channels
 * either way. A section shows only for a context the VOD names, in the VOD's
 * order.
 */
export function cardView(
  member: Pick<CastMember, "aliases" | "account">,
  named: string,
  streamer: Streamer | null,
  contexts: readonly string[],
): CardView {
  const name = streamer?.name ?? named;
  const channels = [...(streamer?.channels ?? [])];
  const own = member.account;
  if (own && !channels.some((c) => accountKey(c) === accountKey(own))) channels.push(own);
  const said = name.trim().toLowerCase();
  const aliases = (streamer?.aliases.length ? streamer.aliases : member.aliases).filter(
    (a) => a.trim() && a.trim().toLowerCase() !== said,
  );
  const sections = streamer
    ? [...new Set(contexts)].flatMap((id) => (streamer.contexts[id] ? [{ id, ...streamer.contexts[id] }] : []))
    : [];
  // The image server has no YouTube pictures to show (CastDisc.svelte): another site's first.
  const pictured = [own, ...channels].find((a) => a && a.platform !== "youtube");
  return {
    name,
    avatar: streamer?.avatar ?? pictured ?? own ?? channels[0] ?? null,
    aliases,
    channels,
    socials: (streamer?.socials ?? []).map((s) => ({ label: socialLabel(s), href: s.url })),
    sections,
  };
}

/**
 * The cards and the index as the file host gives them (`fetchCard`,
 * `fetchIndex`: unread, null when not there), each asked for once a session.
 * A failure to ask (offline) is kept for nothing, so the next card opened
 * asks again; meanwhile the card shows what the VOD knows.
 */
export class CardCache {
  #cards = new Map<string, Promise<Streamer | null>>();
  #index: Promise<unknown> | null = null;

  #fetchCard: (id: string) => Promise<unknown>;
  #fetchIndex: () => Promise<unknown>;

  constructor(fetchCard: (id: string) => Promise<unknown>, fetchIndex: () => Promise<unknown>) {
    this.#fetchCard = fetchCard;
    this.#fetchIndex = fetchIndex;
  }

  /** Card `id`, read; null when it isn't there or can't be had. */
  card(id: string): Promise<Streamer | null> {
    let got = this.#cards.get(id);
    if (!got) {
      const asked = this.#fetchCard(id);
      got = asked.then(
        (value) => readStreamer(value, id),
        () => {
          this.#cards.delete(id);
          return null;
        },
      );
      this.#cards.set(id, got);
    }
    return got;
  }

  /** The index, unread; null when it isn't there or can't be had. */
  index(): Promise<unknown> {
    if (!this.#index) {
      const asked = this.#fetchIndex().catch(() => {
        this.#index = null;
        return null;
      });
      this.#index = asked;
    }
    return this.#index;
  }

  /**
   * The card of `member`: the one the VOD's annotations name, else the one
   * the index has for their account (`account`, the VOD's channel's when the
   * cast gives none), else none.
   */
  async of(member: Pick<CastMember, "streamer" | "account">, account = member.account): Promise<Streamer | null> {
    const named = cardId(member.streamer);
    const id = named ?? (account ? cardIdOf(await this.index(), account) : null);
    return id ? this.card(id) : null;
  }
}
