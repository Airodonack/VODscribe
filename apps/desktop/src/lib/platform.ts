import type { Vod } from "./api";

/** This computer's OS, for shortcuts, drops and install instructions. */
export const os: "windows" | "mac" | "linux" =
  typeof navigator === "undefined" || !/Win|Mac/.test(navigator.platform)
    ? "linux"
    : /Win/.test(navigator.platform)
      ? "windows"
      : "mac";

/** ["a", "b", "c"] -> "a, b and c"; one alone as it is, none "". */
export function listed(items: string[]): string {
  return items.length > 1 ? `${items.slice(0, -1).join(", ")} and ${items.at(-1)}` : (items[0] ?? "");
}

/** 3.4e9 -> "3.4 GB", 9.5e8 -> "950 MB" */
export function formatBytes(bytes: number): string {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`;
}

/** 3725 -> "1:02:05", 65 -> "1:05" */
export function formatTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** From `start` to `end`: 65, 130 -> "1:05–2:10". */
export function formatSpan(start: number, end: number): string {
  return `${formatTime(start)}–${formatTime(end)}`;
}

/** `seconds` to `digits` places, cut (not rounded) as a frame's time should
 *  be, but not just under: frame starts (k/fps) land a hair below. 1:02:03.25. */
export function formatFraction(seconds: number, digits: number): string {
  const scale = 10 ** digits;
  const n = Math.floor(Math.max(0, seconds) * scale + 1e-6);
  return `${formatTime(Math.floor(n / scale))}.${String(n % scale).padStart(digits, "0")}`;
}

/** 3725 -> "1h2m5s", 65 -> "0h1m5s": the format Twitch uses in URLs and its
 *  player, its hours always given. Every Twitch link the app makes uses it. */
export function twitchTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 3600)}h${Math.floor((s % 3600) / 60)}m${s % 60}s`;
}

/** A Twitch video's id as its address has it: yt-dlp prefixes VOD ids with
 *  "v", Twitch's URLs and player don't. Only a "v" before a number goes, so a
 *  clip's slug keeps its own. crates/core annotations.rs `twitch_number` is
 *  the same rule. */
export const twitchNumber = (id: string) => id.replace(/^v(?=\d)/, "");

/** `vod`'s Twitch id as its address has it (`twitchNumber`). */
export const twitchId = (vod: Vod) => twitchNumber(vod.platform_id);

/** A Twitch clip goes by its slug, where a VOD's id is a number (crates/core twitch.rs `is_clip`). */
export const isTwitchClip = (vod: Vod) => vod.platform === "twitch" && !/^v?\d+$/.test(vod.platform_id);

export const platformName: Record<string, string> = {
  youtube: "YouTube",
  twitch: "Twitch",
  kick: "Kick",
  local: "Local file",
};

/** What to call where `vod` is from: the platform's name, or for any other
 *  site yt-dlp knows, its web address ("soundcloud.com"). */
/** What to call something being added (a link or a file path) while it is:
 *  a link without its scheme, a file by its name. */
export function labelOf(item: string): string {
  item = item.trim();
  if (/^https?:\/\//i.test(item)) return item.replace(/^https?:\/\/(www\.)?/i, "");
  return item.split(/[\\/]/).pop() || item;
}

export function siteName(vod: Vod): string {
  const known = platformName[vod.platform];
  if (known) return known;
  try {
    return new URL(vod.url).hostname.replace(/^www\./, "");
  } catch {
    return vod.platform;
  }
}

/** Whether `vod` is from a site whose own player plays it here (YouTube, Twitch, Kick). */
// Both lists are checked against crates/core (pipeline::SITE_PLAYERS,
// settings::DEFAULT_VIDEO_HEIGHT) by its tests.
export const hasSitePlayer = (vod: Vod) => ["youtube", "twitch", "kick"].includes(vod.platform);

/** Whether `vod`'s video is kept to play here: a site with no embed of its
 *  own (crates/core pipeline.rs `keeps_video`). */
export const keepsVideo = (vod: Vod) => !hasSitePlayer(vod) && vod.platform !== "local";

/** Whether `vod` is a file here (on this computer, or its video kept):
 *  its frames are read, and its clips cut, from that, not downloaded. */
export const playsFromFile = (vod: Vod) => !!vod.media || vod.platform === "local";

/** What to try when a video won't play here, for this computer's system. */
export function playbackHint(): string {
  const ua = navigator.userAgent;
  if (/Windows/i.test(ua)) return "Windows can't play this video's format.";
  if (/Mac OS X|Macintosh/i.test(ua)) return "macOS can't play this video's format.";
  return (
    "This computer is missing the codecs for this video's format. On Fedora, install them from RPM Fusion " +
    "(gstreamer1-plugin-libav); on Ubuntu and Debian, install gstreamer1.0-libav. Then restart VODscribe."
  );
}

/** Whether a link to `platform`'s VODs can say a time to start at: not
 *  Kick's (none is documented; it ignores one), which open at the start. */
export const linksHaveTime = (platform: string) => platform !== "kick";

/** Link to the VOD on its own site, at `time` (where its links can say one,
 *  `linksHaveTime`). povs.ts `povUrl` builds another stream's the same way. */
export function watchUrl(vod: Vod, time: number): string {
  const t = Math.max(0, Math.floor(time));
  switch (vod.platform) {
    case "youtube":
      return `https://www.youtube.com/watch?v=${encodeURIComponent(vod.platform_id)}&t=${t}s`;
    case "twitch":
      // A clip's page has no time to start from.
      if (isTwitchClip(vod)) return vod.url;
      return `https://www.twitch.tv/videos/${twitchId(vod)}?t=${twitchTime(t)}`;
    case "kick":
      return vod.url;
    default: {
      const url = new URL(vod.url);
      url.searchParams.set("t", String(t));
      return url.toString();
    }
  }
}

/** What the stamp beside a VOD's title copies, as its tooltip says it: a
 *  link to the moment, or on a site whose links can't say one, the VOD's. */
export function copyLinkTitle(vod: Vod): string {
  return linksHaveTime(vod.platform)
    ? "Copy a link to this moment"
    : `Copy a link to this VOD (${siteName(vod)}'s links can't say a time)`;
}

/** The video heights a download can be capped at (Settings → Storage). */
export const HEIGHTS = [480, 720, 1080];
/** The height downloads are capped at until another is picked (crates/core settings.rs `video_height`). */
export const DEFAULT_HEIGHT = 1080;

/** Roughly how many GB an hour of video takes at each height, as sites encode it. */
const HOUR_GB: Record<number, [number, number]> = { 480: [0.7, 1.2], 720: [1.5, 2.5], 1080: [3, 5] };

/** About how much `hours` of video at `height` takes, as "3 to 5 GB" (or MB, under a GB). */
export function videoSize(height: number, hours = 1): string {
  const [low, high] = (HOUR_GB[height] ?? HOUR_GB[DEFAULT_HEIGHT]).map((gb) => gb * hours);
  if (high < 1) {
    const mb = (gb: number) => Math.max(1, Math.round((gb * 1000) / 10) * 10);
    return `${mb(low)} to ${mb(high)} MB`;
  }
  const round = (gb: number) => String(gb < 10 ? Math.round(gb * 10) / 10 : Math.round(gb));
  return `${round(low)} to ${round(high)} GB`;
}

/**
 * Chapters as YouTube reads them in a video's description: a line each, its
 * start then its title ("0:00 Intro"). YouTube shows them only when the first
 * is at 0:00, there are three or more, and each is at least 10 seconds long.
 */
export function chaptersText(chapters: { start: number; text: string | null }[]): string {
  // A title's own line breaks (one from an annotations file may have them,
  // shown as spaces) would start lines YouTube reads as chapters of their own.
  const title = (text: string | null) => text?.replace(/\s+/g, " ") ?? "Untitled chapter";
  return chapters.map((c) => `${formatTime(c.start)} ${title(c.text)}`).join("\n");
}
