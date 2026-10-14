// What this computer's webview can play, asked at each start and saved
// (crates/core playable.rs), so videos kept from sites without embeds are
// downloaded in a format that plays here. Ranked: formats the hardware
// decodes first (smooth and light on the battery), then any that play, and
// smaller files first within each: AV1, VP9, HEVC, then H.264.

import { api, type Playable } from "./api";
import { DEMO } from "./demo";

type VideoCodec = Playable["video"][number][0];
type AudioCodec = Playable["audio"][number];

// A typical 720p stream of each, as sites label them.
const VIDEO: [VideoCodec, string][] = [
  ["av1", "av01.0.05M.08"],
  ["vp9", "vp09.00.31.08"],
  ["hevc", "hvc1.1.6.L93.B0"],
  ["h264", "avc1.64001F"],
];
// The audio each is usually paired with first.
const PAIRED: Record<VideoCodec, AudioCodec[]> = {
  av1: ["opus", "aac"],
  vp9: ["opus", "aac"],
  hevc: ["aac", "opus"],
  h264: ["aac", "opus"],
};
const IN_MP4: Partial<Record<AudioCodec, string>> = { aac: "mp4a.40.2", opus: "opus" };
// Audio alone (podcasts, music), as files of its own.
const AUDIO: [AudioCodec, string][] = [
  ["opus", 'audio/webm; codecs="opus"'],
  ["aac", 'audio/mp4; codecs="mp4a.40.2"'],
  ["mp3", "audio/mpeg"],
  ["vorbis", 'audio/ogg; codecs="vorbis"'],
  ["flac", "audio/flac"],
];

/** 2: decoded in hardware; 1: plays; 0: doesn't. */
async function rank(contentType: string, audio?: string): Promise<number> {
  const video = document.createElement("video");
  const says = video.canPlayType(contentType) === "probably";
  try {
    const info = await navigator.mediaCapabilities.decodingInfo({
      type: "file",
      ...(audio
        ? { audio: { contentType: audio } }
        : {
            video: { contentType, width: 1280, height: 720, bitrate: 2_500_000, framerate: 30 },
          }),
    });
    if (!info.supported) return says ? 1 : 0;
    return info.powerEfficient && info.smooth ? 2 : 1;
  } catch {
    // No answer (an older webview): what canPlayType says.
    return says ? 1 : 0;
  }
}

/** Asks the webview and saves the answer for downloads. Not in the website's demo. */
export async function savePlayable() {
  if (DEMO) return;
  const pairs: { pair: Playable["video"][number]; score: number }[] = [];
  for (const [codec, label] of VIDEO) {
    const video = await rank(`video/mp4; codecs="${label}"`);
    if (!video) continue;
    for (const audio of PAIRED[codec]) {
      const type = `video/mp4; codecs="${label}, ${IN_MP4[audio]}"`;
      const both = document.createElement("video").canPlayType(type) === "probably";
      if (both || (await rank(`audio/mp4; codecs="${IN_MP4[audio]}"`, `audio/mp4; codecs="${IN_MP4[audio]}"`))) {
        pairs.push({ pair: [codec, audio], score: video });
      }
    }
  }
  // Stable: within a score, the order above (smaller files first).
  pairs.sort((a, b) => b.score - a.score);
  const audio: AudioCodec[] = [];
  for (const [codec, type] of AUDIO) if (await rank(type, type)) audio.push(codec);
  await api.setPlayable({ video: pairs.map((p) => p.pair), audio }).catch((e) => console.error("playable", e));
}
