import type { Vod } from "./api";

/** This computer's OS, for shortcuts, drops and install instructions. */
export const os: "windows" | "mac" | "linux" =
  typeof navigator === "undefined" || !/Win|Mac/.test(navigator.platform)
    ? "linux"
    : /Win/.test(navigator.platform)
      ? "windows"
      : "mac";

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

/** 3725 -> "1h2m5s", the format Twitch uses in URLs and its player. */
export function twitchTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 3600)}h${Math.floor((s % 3600) / 60)}m${s % 60}s`;
}

/** yt-dlp prefixes Twitch VOD ids with "v"; Twitch's URLs and player don't. */
export const twitchId = (vod: Vod) => vod.platform_id.replace(/^v/, "");

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
export const hasSitePlayer = (vod: Vod) => ["youtube", "twitch", "kick"].includes(vod.platform);

/** Whether `vod`'s video is kept to play here: a site with no embed of its
 *  own (crates/core pipeline.rs `keeps_video`). */
export const keepsVideo = (vod: Vod) => !hasSitePlayer(vod) && vod.platform !== "local";

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

/** Link to the VOD on its own site, at `time`. */
export function watchUrl(vod: Vod, time: number): string {
  const t = Math.floor(time);
  switch (vod.platform) {
    case "youtube":
      return `https://www.youtube.com/watch?v=${encodeURIComponent(vod.platform_id)}&t=${t}s`;
    case "twitch":
      // A clip's page has no time to start from.
      if (isTwitchClip(vod)) return vod.url;
      return `https://www.twitch.tv/videos/${twitchId(vod)}?t=${twitchTime(t)}`;
    default: {
      const url = new URL(vod.url);
      url.searchParams.set("t", String(t));
      return url.toString();
    }
  }
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
