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

export const platformName: Record<string, string> = {
  youtube: "YouTube",
  twitch: "Twitch",
  kick: "Kick",
  local: "Local file",
};

/** Link to the VOD on its own site, at `time`. */
export function watchUrl(vod: Vod, time: number): string {
  const t = Math.floor(time);
  switch (vod.platform) {
    case "youtube":
      return `https://www.youtube.com/watch?v=${encodeURIComponent(vod.platform_id)}&t=${t}s`;
    case "twitch":
      return `https://www.twitch.tv/videos/${twitchId(vod)}?t=${twitchTime(t)}`;
    default: {
      const url = new URL(vod.url);
      url.searchParams.set("t", String(t));
      return url.toString();
    }
  }
}
