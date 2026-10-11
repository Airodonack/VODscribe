import type { Vod } from "./api";

/** 3725 -> "1:02:05", 65 -> "1:05" */
export function formatTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** 3725 -> "1h2m5s", the format Twitch uses in URLs. */
function twitchTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 3600)}h${Math.floor((s % 3600) / 60)}m${s % 60}s`;
}

const twitchId = (vod: Vod) => vod.platform_id.replace(/^v/, "");

export const platformName: Record<string, string> = {
  youtube: "YouTube",
  twitch: "Twitch",
  kick: "Kick",
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
