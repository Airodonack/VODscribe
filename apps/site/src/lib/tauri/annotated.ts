// A VOD with annotations (desktop annotations.ts), as the site's per-VOD page
// (/v/<platform>/<id>) shows it: where its two files come from, and the
// annotations turned into the demo's shapes, so the stand-ins answer the app's
// commands from them as they do from static/demo. Only types and the download
// host are imported, so Node can test it directly (annotated.test.mjs).

import type { ChatLine, Vod } from "desktop/lib/api.ts";
import type { Annotations } from "desktop/lib/annotations.ts";
import { DOWNLOAD_HOST } from "../downloads.ts";

/** A line as the demo's transcripts have it: text, start, each word's start; and here, its end and speaker. */
export type Segment = [text: string, start: number, wordStarts: number[], end?: number, speaker?: string];

/** The platforms a VOD page can be for. */
export const PLATFORMS = ["twitch", "youtube", "kick"];

/** Where a VOD's annotations and chat are fetched from. */
export interface Source {
  annotations: string;
  chat: string;
  /** From `?src=`, rather than the download host. */
  custom: boolean;
}

/**
 * A VOD's id as the app stores it and the host's files are named: Twitch's
 * with its "v" (a page's address can leave it out, as Twitch's own do).
 */
export function storedId(platform: string, id: string): string {
  return platform === "twitch" && /^\d+$/.test(id) ? `v${id}` : id;
}

/** A VOD listed in the download host's annotations/index.json. */
export interface IndexEntry {
  platform: string;
  id: string;
  channel: string;
  title: string;
  duration: number;
  produced: { at: string };
}

/** The host's index of VODs: the entries with a platform and id, newest first. */
export function indexEntries(value: unknown): IndexEntry[] {
  if (!Array.isArray(value)) throw new Error("The list of VODs couldn't be read.");
  return value
    .filter((e) => typeof e?.platform === "string" && typeof e?.id === "string")
    .sort((a, b) => String(b.produced?.at ?? "").localeCompare(String(a.produced?.at ?? "")));
}

/** A VOD's page on this site: Twitch's id as Twitch writes it, without the "v". */
export const pageOf = (platform: string, id: string) =>
  `/v/${encodeURIComponent(platform)}/${encodeURIComponent(platform === "twitch" ? id.replace(/^v(?=\d+$)/, "") : id)}`;

/** The chat file beside an annotations file at `url`: its name with .chat.json for .json. */
export const chatBeside = (url: string) => {
  const at = new URL(url);
  at.pathname = `${at.pathname.replace(/\.json$/, "")}.chat.json`;
  return at.toString();
};

/**
 * Where the page at /v/<platform>/<id> reads from: the URL in `?src=` (and
 * the chat beside it), resolved against the page, or else the download host,
 * where the app looks too (crates/core annotations.rs `file_url`). Null for a
 * platform the page doesn't know.
 */
export function pickSource(
  platform: string,
  id: string,
  search: { get(name: string): string | null },
  page: string,
): Source | null {
  const src = search.get("src");
  if (src) {
    const annotations = new URL(src, page).toString();
    return { annotations, chat: chatBeside(annotations), custom: true };
  }
  if (!PLATFORMS.includes(platform) || !id) return null;
  const path = `annotations/${encodeURIComponent(platform)}/${encodeURIComponent(storedId(platform, id))}`;
  return { annotations: `${DOWNLOAD_HOST}/${path}.json`, chat: `${DOWNLOAD_HOST}/${path}.chat.json`, custom: false };
}

/**
 * Which of the files dropped (or picked) is the annotations and which the
 * chat: the one named .chat.json is the chat, any other the annotations.
 */
export function pickDropped<F extends { name: string }>(
  files: F[],
): { annotations: F; chat: F | null } | { error: string } {
  const chat = files.find((f) => /\.chat\.json$/i.test(f.name)) ?? null;
  const rest = files.filter((f) => f !== chat);
  if (rest.length > 1) return { error: "Drop one annotations file, and its .chat.json beside it if you have it." };
  if (!rest.length)
    return { error: chat ? "That's a chat file: drop its annotations file with it." : "No file was dropped." };
  return { annotations: rest[0], chat };
}

/**
 * The transcript in the demo's shape. A line's words are its text split on
 * spaces (the backend reads them so), each with its start: the file's words'
 * starts where they're as many, else the nearest word's by position (a
 * corrected line may have more or fewer words than were timed).
 */
export function segmentsOf(annotations: Annotations): Segment[] {
  return [...annotations.transcript.segments]
    .sort((a, b) => a.start - b.start)
    .map((seg) => {
      const tokens = seg.text.split(" ").filter(Boolean);
      const timed = seg.words ?? [];
      const starts = !timed.length
        ? []
        : tokens.length === timed.length
          ? timed.map((w) => w.start)
          : tokens.map((_, j) => timed[Math.round((j * (timed.length - 1)) / Math.max(1, tokens.length - 1))].start);
      const line: Segment = [tokens.join(" "), seg.start, starts, seg.end];
      if (seg.speaker) line.push(seg.speaker);
      return line;
    });
}

/**
 * The chat file's messages (the app's ChatMessage, chat.rs) as the chat
 * commands give them: numbered, in time order, with the fields the app's
 * serializer fills in. The error's message is for the user.
 */
export function chatOf(value: unknown): ChatLine[] {
  if (!Array.isArray(value)) throw new Error("This isn't a VODscribe chat file: not a list of messages.");
  return value
    .filter((m) => typeof m?.time === "number" && Array.isArray(m?.parts))
    .sort((a, b) => a.time - b.time)
    .map((m, i) => ({
      id: i + 1,
      time: m.time,
      author: String(m.author ?? ""),
      color: m.color ?? null,
      parts: m.parts,
      kind: m.kind ?? "message",
      amount: m.amount ?? null,
    }));
}

/** The platform's page for a VOD, as the app keeps it in `Vod.url`. */
function pageUrl(platform: string, id: string, channel: string): string {
  if (platform === "twitch") return `https://www.twitch.tv/videos/${id.replace(/^v/, "")}`;
  if (platform === "youtube") return `https://www.youtube.com/watch?v=${encodeURIComponent(id)}`;
  if (platform === "kick") return `https://kick.com/${encodeURIComponent(channel)}/videos/${encodeURIComponent(id)}`;
  return "";
}

/** The library's one VOD, transcribed whole, its chat as `chat` says. */
export function vodOf(annotations: Annotations, id: number, chat: Vod["chat"]): Vod {
  const s = annotations.source;
  return {
    id,
    platform: s.platform,
    platform_id: s.id,
    url: pageUrl(s.platform, s.id, s.channel ?? ""),
    title: s.title || s.id,
    channel: s.channel || null,
    duration: s.duration,
    thumbnail: null,
    thumb: "",
    source_path: null,
    media: false,
    audio_only: false,
    published: s.recorded_at?.slice(0, 10) ?? null,
    // The app keeps the file's time as the VOD's start when nothing better says (crates/core `db::MIGRATE_V33`).
    started_at: s.recorded_at ?? null,
    status: "done",
    progress: 1,
    error: null,
    stretch: null,
    transcribed: "all",
    waiting: 0,
    added_at: 0,
    note: null,
    outdated: false,
    chat,
    chat_error: null,
    listed: true,
  };
}
