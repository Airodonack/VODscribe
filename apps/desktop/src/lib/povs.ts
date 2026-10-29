// Another point of view: when a cast member's own stream ran alongside this
// VOD (annotations.ts `Pov`), their name and disc link to that stream at the
// same moment rather than to their channel (castLink.svelte.ts): the moment
// is this VOD's start (its `started_at`) plus how far into it, less theirs.
// Pure, so Node tests it (povs.test.mjs).

import type { Account, Pov, Video } from "./annotations";
import { formatTime, linksHaveTime, twitchNumber, twitchTime } from "./platform";

/** `iso` as Unix seconds; null when there's none or it isn't a time. */
export function unixTime(iso: string | null | undefined): number | null {
  const ms = iso ? Date.parse(iso) : NaN;
  return Number.isNaN(ms) ? null : ms / 1000;
}

/** Whether two accounts are one: the same site, the login in any case. */
export const sameAccount = (a: Account, b: Account): boolean =>
  a.platform === b.platform && a.login.trim().toLowerCase() === b.login.trim().toLowerCase();

/**
 * `account`'s stream running at `at` (Unix seconds), of `povs`: of two (a
 * stream restarted), the later started; null when none was.
 */
export function povAt(povs: Pov[], account: Account, at: number): Pov | null {
  let found: Pov | null = null;
  let foundStart = -Infinity;
  for (const pov of povs) {
    const start = unixTime(pov.start);
    if (start === null || !sameAccount(pov.account, account)) continue;
    if (at >= start && at <= start + pov.duration && start > foundStart) {
      found = pov;
      foundStart = start;
    }
  }
  return found;
}

/** How far into `pov` the moment `at` (Unix seconds) is, in seconds, kept inside it. */
export function povOffset(pov: Pov, at: number): number {
  const start = unixTime(pov.start) ?? at;
  return Math.min(Math.max(at - start, 0), pov.duration);
}

/** A time as the app shows it: 5025 → "1:23:45", 65 → "1:05" (platform.ts `formatTime`). */
export const clock = formatTime;

/** The site a video is on, as it's read out: "twitch.tv". */
export const siteLabel = (platform: Video["platform"]): string =>
  platform === "twitch" ? "twitch.tv" : platform === "kick" ? "kick.com" : "youtube.com";

/**
 * Where `pov` plays from `offset` seconds in, on its site, as platform.ts
 * `watchUrl` links a VOD in the library. A Kick video's address has no time
 * of its own (`linksHaveTime`): it opens at the start.
 */
export function povUrl(pov: Pov, offset: number): string {
  const { platform, id } = pov.video;
  const t = Math.max(0, Math.floor(offset));
  const part = encodeURIComponent;
  switch (platform) {
    case "twitch":
      return `https://www.twitch.tv/videos/${part(twitchNumber(id))}?t=${twitchTime(t)}`;
    case "kick":
      return `https://kick.com/${part(pov.account.login.toLowerCase())}/videos/${part(id)}`;
    case "youtube":
      return `https://www.youtube.com/watch?v=${part(id)}&t=${t}s`;
  }
}

/** Whether the library's `vod` is `video`: the same site and id, a Twitch id with or without its "v". */
export function isVideo(vod: { platform: string; platform_id: string }, video: Video): boolean {
  if (vod.platform !== video.platform) return false;
  return video.platform === "twitch"
    ? twitchNumber(vod.platform_id) === twitchNumber(video.id)
    : vod.platform_id === video.id;
}

/** Where a link to another point of view goes: whose stream, how far into it, its address, and its tooltip. */
export type PovLink = { pov: Pov; offset: number; url: string; title: string };

/**
 * What a link to `pov` at `offset` seconds says it does: "Burn's stream at
 * 1:23:45 · twitch.tv"; `here` when the library has it, which opens there.
 */
export function povTitle(pov: Pov, offset: number, here = false): string {
  const where = here
    ? "in your library"
    : !linksHaveTime(pov.video.platform)
      ? "kick.com, from its start"
      : siteLabel(pov.video.platform);
  return `${pov.player}'s stream at ${clock(offset)} · ${where}`;
}

/**
 * Where `account`'s link goes at `moment` seconds into a VOD that started at
 * `started`: their stream running then, at the same moment; null when none
 * was, or when this VOD's start isn't known (their channel, then).
 */
export function povLink(
  povs: Pov[],
  account: Account,
  started: string | null | undefined,
  moment: number | null,
): PovLink | null {
  const start = unixTime(started);
  if (start === null || moment === null || !Number.isFinite(moment)) return null;
  const at = start + moment;
  const pov = povAt(povs, account, at);
  if (!pov) return null;
  const offset = povOffset(pov, at);
  return { pov, offset, url: povUrl(pov, offset), title: povTitle(pov, offset) };
}
