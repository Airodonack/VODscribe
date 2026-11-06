// What an imported annotations file (annotations.ts) shows, worked out for
// the views: the colours its cast and activity are drawn in, and what's said
// of them (a highlight's kind is an emoji, annotations.ts `highlightGlyph`;
// chapters have no colour of their own). The data comes through the app's
// commands (list_marks, transcript, vod_annotations), never from the file.

import { channelLabel, channelUrl } from "./accounts";
import type { Activity, CastMember, Vod } from "./api";
import { sameName } from "./castNames";

/** The cast's colours, by their place in the cast: each reads on the light
 *  and the dark theme alike, and none is the accent. */
export const CAST_COLORS = ["#2f7de1", "#d9534f", "#1f9d6b", "#c27c0e", "#8a5cd6", "#0e9aa7", "#c2408a", "#6b8e23"];

/** What the VOD is doing, by kind; another kind is gray. */
export const ACTIVITY_COLORS: Record<string, string> = {
  game: "#2f7de1",
  "just chatting": "#1f9d6b",
  event: "#db2777",
  break: "#8c8c86",
};

const OTHER = "#8c8c86";

/** The place in `cast` of whoever `name` is: by their name or one of their aliases, any case; -1 if none. */
export function castIndex(cast: CastMember[], name: string | null | undefined): number {
  const key = name?.trim().toLowerCase();
  if (!key) return -1;
  return cast.findIndex((m) => m.name.toLowerCase() === key || m.aliases.some((a) => a.toLowerCase() === key));
}

/** The colour of `name`'s lines, by their place in the cast; null for someone not in it. */
export function speakerColor(cast: CastMember[], name: string | null | undefined): string | null {
  const i = castIndex(cast, name);
  return i < 0 ? null : CAST_COLORS[i % CAST_COLORS.length];
}

/** An activity's colour, by its kind. */
export const activityColor = (kind: string): string => ACTIVITY_COLORS[kind.toLowerCase()] ?? OTHER;

/** Words with their first letter a capital: "just chatting" → "Just chatting". */
const sentence = (s: string) => (s ? s[0].toUpperCase() + s.slice(1) : s);

/** What an activity says on the timeline: "Game: Chess", "Just chatting". */
export function activityLabel(a: Activity): string {
  const name = a.name?.trim();
  return name ? `${sentence(a.kind)}: ${name}` : sentence(a.kind);
}

/** Whether a cast member is a text-to-speech voice (role "tts"): no one's channel, so never a link. */
export const isVoice = (m: Pick<CastMember, "role"> | undefined): boolean => m?.role.trim().toLowerCase() === "tts";

/** A role as the cast list says it: "Streamer", "In-game character"; a text-to-speech voice's "TTS". */
const roleLabel = (role: string) => (role.trim().toLowerCase() === "tts" ? "TTS" : sentence(role));

/** A cast member's role and aliases, as the cast list says them: "Streamer · also Mar, M". An alias
 *  that is their name, or their player's (already in the name they're listed by, castNames.ts), isn't repeated. */
export function castDetail(m: CastMember): string {
  const said = [m.name, m.player ?? ""].map((n) => n.trim().toLowerCase());
  const aliases = m.aliases.filter((a) => a.trim() && !said.includes(a.trim().toLowerCase()));
  return [roleLabel(m.role), aliases.length ? `also ${aliases.join(", ")}` : ""].filter(Boolean).join(" · ");
}

/**
 * The cast member whose channel the VOD is on, by the channel's name: the one
 * with an account whose name, player or login is `channel`, or whose login is
 * the channel's `login` as its site gave it (any case), a streamer first.
 * Their picture goes beside the channel's name in the VOD's heading and its
 * library row. Undefined when none has an account.
 */
export function channelMember(
  cast: CastMember[],
  channel: string | null | undefined,
  login?: string | null,
): CastMember | undefined {
  const lower = (n: string | null | undefined) => n?.trim().toLowerCase() || undefined;
  const [key, loginKey] = [lower(channel), lower(login)];
  if (!key && !loginKey) return undefined;
  const found = cast.filter(
    (m) =>
      m.account &&
      !isVoice(m) &&
      ([m.name, m.player, m.account.login].some((n) => key && lower(n) === key) ||
        (loginKey && lower(m.account.login) === loginKey)),
  );
  return found.find((m) => m.role.trim().toLowerCase() === "streamer") ?? found[0];
}

/**
 * Who VOD `vod`'s channel is, in its cast `cast`: the cast member the
 * channel is by an account (`channelMember`, by the channel's name or the
 * login its site gave), else one named as it (by name or player) with no
 * account, given the channel's own where the VOD has one
 * (`channelAccount`), else the channel itself, with that account. The
 * picture beside the channel's name (ChannelDisc.svelte) and the card it
 * opens (streamers.svelte.ts `channelCard`) are this one person. Undefined
 * for a VOD with no channel.
 */
export function channelPerson(
  vod: Pick<Vod, "platform" | "channel" | "url" | "channel_login" | "channel_url">,
  cast: CastMember[],
): CastMember | undefined {
  const channel = vod.channel?.trim();
  if (!channel) return undefined;
  const own = channelAccount(vod);
  const account = own ? { account: own } : {};
  const named = cast.find(
    (m) => !isVoice(m) && (sameName(m.name, channel) || (!!m.player && sameName(m.player, channel))),
  );
  return (
    channelMember(cast, channel, vod.channel_login) ??
    (named && { ...named, ...account }) ?? { name: vod.channel!, aliases: [], role: "streamer", ...account }
  );
}

/**
 * VOD `vod`'s own channel as an account, from the VOD itself, for when its
 * annotations give its streamer none: on Twitch the login the site gave
 * (`channel_login`), else, for a VOD added before that was kept, the
 * channel's name in lower case (when that name can be a login); on Kick
 * it's in the VOD's address (kick.com/<login>/videos/…); on YouTube the
 * handle the site gave ("@name") or the channel's id, else either as its
 * channel's page (`channel_url`) has it. None for a file, or when nothing
 * says (never a guess from YouTube's name).
 */
export function channelAccount(
  vod: Pick<Vod, "platform" | "channel" | "url" | "channel_login" | "channel_url">,
): CastMember["account"] {
  if (vod.platform === "twitch") {
    const login = (vod.channel_login || vod.channel)?.trim().toLowerCase();
    return login && /^[a-z0-9_]{1,25}$/.test(login) ? { platform: "twitch", login } : undefined;
  }
  if (vod.platform === "kick") {
    const login = /^https:\/\/(?:www\.)?kick\.com\/([A-Za-z0-9_-]+)\/videos\//.exec(vod.url)?.[1];
    return login ? { platform: "kick", login: login.toLowerCase() } : undefined;
  }
  if (vod.platform === "youtube") {
    const given = vod.channel_login?.trim();
    if (given && (YOUTUBE_HANDLE.test(given) || YOUTUBE_CHANNEL_ID.test(given)))
      return { platform: "youtube", login: given };
    const page = vod.channel_url ? httpsUrl(vod.channel_url) : null;
    if (!page || siteOf(page) !== "youtube.com") return undefined;
    const [, first, second, ...rest] = page.pathname.replace(/\/+$/, "").split("/");
    if (rest.length) return undefined;
    const handle = first && !second ? decoded(first) : null;
    if (handle && YOUTUBE_HANDLE.test(handle)) return { platform: "youtube", login: handle };
    if (first === "channel" && second && YOUTUBE_CHANNEL_ID.test(second)) return { platform: "youtube", login: second };
  }
  return undefined;
}

/** A YouTube handle as yt-dlp gives it, "@" first; YouTube's allow letters of any script. */
const YOUTUBE_HANDLE = /^@[\p{L}\p{M}\p{N}._·-]{1,100}$/u;
/** A YouTube channel's id. */
const YOUTUBE_CHANNEL_ID = /^UC[A-Za-z0-9_-]{22}$/;

const decoded = (part: string): string | null => {
  try {
    return decodeURIComponent(part);
  } catch {
    return null;
  }
};

/** `url` read as an https link to a plain host (no user, password or port); null for anything else. */
function httpsUrl(url: string): URL | null {
  let read: URL;
  try {
    read = new URL(url);
  } catch {
    return null;
  }
  const plain = read.protocol === "https:" && !read.username && !read.password && !read.port;
  return plain && read.hostname.includes(".") ? read : null;
}

/** The site `url` is on: its host without "www." or "m.". */
const siteOf = (url: URL) => url.hostname.replace(/^(?:www|m)\./, "");

/**
 * VOD `vod`'s own channel as a link on its streamer card, from the VOD
 * itself: on YouTube its account's (`channelAccount`), on another site the
 * channel's page as the site gave it (`channel_url`), only when that is
 * https on the VOD's own site (its host or one under it). None on Twitch
 * and Kick, whose card goes by the account as before, nor for a file, nor
 * when nothing is known.
 */
export function channelPage(
  vod: Pick<Vod, "platform" | "channel" | "url" | "channel_login" | "channel_url">,
): { label: string; href: string } | undefined {
  if (vod.platform === "twitch" || vod.platform === "kick" || vod.platform === "local") return undefined;
  if (vod.platform === "youtube") {
    const account = channelAccount(vod);
    return account && { label: channelLabel(account), href: channelUrl(account) };
  }
  const page = vod.channel_url ? httpsUrl(vod.channel_url) : null;
  const own = vod.url ? httpsUrl(vod.url) : null;
  if (!page || !own) return undefined;
  const [site, vods] = [siteOf(page), siteOf(own)];
  if (site !== vods && !site.endsWith(`.${vods}`)) return undefined;
  return { label: page.href.replace(/^https:\/\/(?:www\.)?/, "").replace(/\/$/, ""), href: page.href };
}

/**
 * Highlights close together on a timeline, as one glyph each: walking them
 * in order of `x` (where each is drawn, in pixels), one less than `within`
 * from the first of the group before joins it. The first of each group is
 * the one drawn (and what a click goes to); `count` says how many it stands
 * for, and `members` which (the first among them). Zoomed in, they come apart.
 */
export function crowds<T>(
  items: T[],
  x: (item: T) => number,
  within: number,
): { first: T; count: number; members: T[] }[] {
  const sorted = items.map((item) => ({ item, at: x(item) })).sort((a, b) => a.at - b.at);
  const out: { first: T; count: number; members: T[]; at: number }[] = [];
  for (const { item, at } of sorted) {
    const last = out.at(-1);
    if (last && at - last.at < within) {
      last.count++;
      last.members.push(item);
    } else out.push({ first: item, count: 1, members: [item], at });
  }
  return out.map(({ first, count, members }) => ({ first, count, members }));
}
