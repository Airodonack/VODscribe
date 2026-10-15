import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Status = "queued" | "downloading" | "ready" | "transcribing" | "paused" | "done" | "error";

export interface Vod {
  id: number;
  platform: string;
  platform_id: string;
  url: string;
  title: string;
  channel: string | null;
  duration: number | null;
  thumbnail: string | null;
  /** File name of the local thumbnail copy ("" if there's none); see `thumbUrl`. */
  thumb: string | null;
  /** For a file on this computer, its path; such VODs have platform "local". */
  source_path: string | null;
  /** A video kept to play here, from a site without an embed (crates/core `Vod::media`). */
  media: boolean;
  /** What plays here is audio alone (by its file type): no picture to show. */
  audio_only: boolean;
  published: string | null;
  status: Status;
  progress: number;
  error: string | null;
  added_at: number;
  /** Not an error: e.g. which device transcribed it, or why the GPU wasn't used. */
  note: string | null;
  /** Has transcript from a model this version no longer uses; unfinished ones start over on resume. */
  outdated: boolean;
  /** Where its chat replay is: not fetched yet (or fetching), fetched, none on the site, or failed. */
  chat: "pending" | "done" | "none" | "failed";
}

export type Device = "cpu" | "gpu";

export type ThemeChoice = "system" | "light" | "dark";

export interface Settings {
  device: Device;
  theme: ThemeChoice;
  /** Where chat's word emotes come from, in priority order (crates/core `settings::EmoteProviders`). */
  emote_providers: EmoteProvider[];
  /** The tallest video kept from sites without embeds; unset means 720. */
  video_height: number | null;
}

/** What this computer's webview plays, best first (crates/core `playable::Playable`). */
export interface Playable {
  video: ["av1" | "vp9" | "hevc" | "h264", "opus" | "aac" | "mp3" | "vorbis" | "flac"][];
  audio: ("opus" | "aac" | "mp3" | "vorbis" | "flac")[];
}

/** A word emote provider (crates/core `word_emotes`): "7tv", "bttv" or "ffz". */
export interface EmoteProvider {
  id: string;
  on: boolean;
}

export interface GpuStatus {
  supported: boolean;
  installed: boolean;
  installing: boolean;
  /** While installing, the latest progress (as the last `gpuProgress` event had it). */
  progress: DownloadProgress | null;
  worker: boolean;
  gpu: { name: string; total_mb: number; free_mb: number } | null;
  min_free_mb: number;
  /** Bytes the add-on downloads. */
  download_bytes: number;
}

export interface SearchHit {
  vod_id: number;
  /** What was said, or what chat wrote. */
  source: "speech" | "chat";
  /** The segment's or chat message's id; unique within its source. */
  id: number;
  time: number;
  parts: { text: string; hit: boolean }[];
  /** For chat, who wrote it, and their name color. */
  author: string | null;
  color: string | null;
}

/** What a search looks through (crates/core `SearchFilter`). Fields left out don't filter. */
export interface SearchFilter {
  /** Only this VOD. */
  vodId?: number | null;
  speech?: boolean;
  chat?: boolean;
}

export interface SetupStatus {
  yt_dlp: boolean;
  /** yt-dlp is the app's own copy, verified when downloaded (not one found on PATH). */
  yt_dlp_managed: boolean;
  ffmpeg: boolean;
  ffmpeg_managed: boolean;
  js_runtime_managed: boolean;
  /** Setup can download ffmpeg and Deno on this platform. */
  can_install_tools: boolean;
  /** Bytes of the speech model setup downloads. */
  model_bytes: number;
  model: boolean;
  js_runtime: boolean;
  running: boolean;
}

/** Where a download is (crates/core `Progress`). Sizes are null while unknown. */
export interface DownloadProgress {
  /** "Downloading <exact name>", then "<name> matches its SHA-256". */
  step: string;
  /** Bytes so far across the whole install, and its size. */
  done: number;
  total: number | null;
  /** Bytes of the file under way, and its size. */
  file_done: number;
  file_size: number | null;
  /** Which of the install's files this is, from 1, out of how many. */
  part: number;
  parts: number;
}

export interface Line {
  start: number;
  /** Missing in the website's demo, whose lines end where the next begins. */
  end?: number;
  text: string;
}

export type ClipStatus = "waiting" | "cutting" | "ready" | "error";

/** A range of a VOD kept as a video file of its own (crates/core `db::Clip`). */
export interface Clip {
  id: number;
  vod_id: number;
  start: number;
  end: number;
  title: string;
  status: ClipStatus;
  file_path: string | null;
  error: string | null;
}

/** A mark on a VOD's timeline (crates/core `db::Mark`): a cut, between two frames. */
export interface Mark {
  id: number;
  vod_id: number;
  kind: string;
  start: number;
}

/** A caption (crates/core `transcript::Cue`). */
export interface Cue {
  start: number;
  end: number;
  text: string;
}

/** A run of a chat message (crates/core `ChatPart`). */
export type ChatPart = { type: "text"; text: string } | { type: "emote"; name: string; url: string };

/** What the transcript view needs of a VOD's chat as a whole (desktop `chats::ChatOverview`). */
export interface ChatOverview {
  count: number;
  histogram: number[];
  /** The subs, gifts and tips. */
  events: ChatLine[];
}

/** A stretch of chat: its first messages and how many it has (crates/core `db::ChatWindow`). */
export interface ChatWindow {
  messages: ChatLine[];
  total: number;
}

/** A saved chat search drawn on the timeline (crates/core `layers`). */
export interface Layer {
  id: string;
  name: string;
  /** `#rrggbb`. */
  color: string;
  terms: { text: string; emote: string | null }[];
  visible: boolean;
  /** Only this channel's VODs; null for every channel. */
  channel: { platform: string; channel: string } | null;
}

/** What a chat message is: someone writing, or an event the site posts in chat (crates/core `chat::Kind`). */
export type ChatKind = "message" | "sub" | "gift" | "tip";

/** The timeline's layers and the chat events drawn on it (crates/core `layers::Timeline`). */
export interface TimelineSettings {
  layers: Layer[];
  events: Exclude<ChatKind, "message">[];
}

/** An emote chat writes as a word: 7TV, BTTV, FFZ (crates/core `word_emotes::WordEmote`). */
export interface WordEmote {
  name: string;
  url: string;
  /** Drawn over the emote before it, not beside it. */
  overlay: boolean;
}

export interface ChatLine {
  /** Unique in the library; what `chatMatches` returns. */
  id: number;
  /** Seconds into the VOD. */
  time: number;
  author: string;
  /** `#rrggbb`, if the site gives one. */
  color: string | null;
  parts: ChatPart[];
  kind: ChatKind;
  /** For a gift, how many; for a tip, how many bits or KICKs. */
  amount: number | null;
}

export const api = {
  setupStatus: () => invoke<SetupStatus>("setup_status"),
  runSetup: () => invoke<void>("run_setup"),
  /** Stops setup's downloads; tools and model files that finished stay. `setupDone` then fires with null. */
  cancelSetup: () => invoke<void>("cancel_setup"),
  /** False for a copy that wasn't installed from a package, which has no update to fetch. */
  selfUpdatable: () => invoke<boolean>("self_updatable"),
  dataDir: () => invoke<string>("data_dir"),
  openDataDir: () => invoke<void>("open_data_dir"),
  /** Adds a link; `ticket` lets cancelAdd stop it while it's looked up. */
  addVod: (url: string, ticket?: number) => invoke<{ vod: Vod; created: boolean }>("add_vod", { url, ticket }),
  cancelAdd: (ticket: number) => invoke<void>("cancel_add", { ticket }),
  addFiles: (paths: string[]) => invoke<{ vods: Vod[]; errors: string[] }>("add_files", { paths }),
  /** The file extensions VODscribe accepts, for the file picker. */
  mediaExtensions: () => invoke<string[]>("media_extensions"),
  showFile: (id: number) => invoke<void>("show_file", { id }),
  openFile: (id: number) => invoke<void>("open_file", { id }),
  mediaUrl: (id: number) => invoke<string>("media_url", { id }),
  /** Loudness, ten levels a second (crates/core `peaks`); empty if unknown. */
  peaks: (id: number) => invoke<ArrayBuffer>("peaks", { id }).then((b) => new Uint8Array(b)),
  frameRate: (id: number) => invoke<number | null>("frame_rate", { id }),
  captions: (vodId: number) => invoke<Cue[]>("captions", { vodId }),
  listClips: (vodId: number) => invoke<Clip[]>("list_clips", { vodId }),
  listMarks: (vodId: number) => invoke<Mark[]>("list_marks", { vodId }),
  addMark: (vodId: number, time: number) => invoke<Mark>("add_mark", { vodId, time }),
  moveMark: (id: number, time: number) => invoke<void>("move_mark", { id, time }),
  deleteMark: (id: number) => invoke<void>("delete_mark", { id }),
  addClip: (vodId: number, start: number, end: number) => invoke<Clip>("add_clip", { vodId, start, end }),
  setClipRange: (id: number, start: number, end: number) => invoke<Clip>("set_clip_range", { id, start, end }),
  renameClip: (id: number, title: string) => invoke<Clip>("rename_clip", { id, title }),
  deleteClip: (id: number) => invoke<void>("delete_clip", { id }),
  showClip: (id: number) => invoke<void>("show_clip", { id }),
  /** Starts dragging clip `id`'s file out of the window; call while the mouse button is down. */
  dragClip: (id: number) => invoke<void>("drag_clip", { id }),
  /** Saves a copy of clip `id`'s file where the person picks; false if they canceled. */
  saveClip: (id: number) => invoke<boolean>("save_clip", { id }),
  /** Downloads VOD `id`'s whole video to play here (YouTube, Twitch, Kick). */
  keepVideo: (id: number, height?: number) => invoke<void>("keep_video", { id, height }),
  /** Stops (and discards) VOD `id`'s video download. */
  stopVideo: (id: number) => invoke<void>("stop_video", { id }),
  /** VODs whose video is downloading or waiting to. */
  videoDownloads: () => invoke<number[]>("video_downloads"),
  /** Deletes every downloaded video; returns how many were kept, still needed to transcribe. */
  deleteAllVideos: () => invoke<number>("delete_all_videos"),
  openVideosDir: () => invoke<void>("open_videos_dir"),
  listVods: () => invoke<Vod[]>("list_vods"),
  reorderVods: (ids: number[]) => invoke<void>("reorder_vods", { ids }),
  deleteVod: (id: number) => invoke<void>("delete_vod", { id }),
  retryVod: (id: number) => invoke<void>("retry_vod", { id }),
  cancelVod: (id: number) => invoke<void>("cancel_vod", { id }),
  retranscribeVod: (id: number) => invoke<void>("retranscribe_vod", { id }),
  pauseVod: (id: number) => invoke<void>("pause_vod", { id }),
  resumeVod: (id: number) => invoke<void>("resume_vod", { id }),
  search: (query: string, filter: SearchFilter) =>
    invoke<{ hits: SearchHit[]; truncated: boolean }>("search", { query, filter }),
  /** A URL that plays Twitch clip `slug`'s video, signed for a while. */
  twitchClipVideo: (slug: string) => invoke<string>("twitch_clip_video", { slug }),
  /** The transcript's lines; with `after`, only those starting after it. */
  transcript: (vodId: number, after?: number) => invoke<Line[]>("transcript", { vodId, after }),
  /** A VOD's chat count, messages per each of `bins` stretches of `duration`, and its
   *  events; `events: false` leaves those out (`events` is then null). */
  chatOverview: (vodId: number, duration: number, bins: number, events = true) =>
    invoke<ChatOverview & { events: ChatLine[] | null }>("chat_overview", { vodId, duration, bins, events }),
  /** For each [from, to) in seconds, its first `cap` messages and how many it has. */
  chatWindows: (vodId: number, ranges: [number, number][], cap: number) =>
    invoke<ChatWindow[]>("chat_windows", { vodId, ranges, cap }),
  /** The emotes in a VOD's chat, as [name, image URL]. */
  chatEmotes: (vodId: number) => invoke<[string, string][]>("chat_emotes", { vodId }),
  /** `providers`: the ids turned on, in priority order. */
  chatWordEmotes: (vodId: number, providers: string[]) => invoke<WordEmote[]>("chat_word_emotes", { vodId, providers }),
  getTimeline: () => invoke<TimelineSettings>("get_timeline"),
  setTimeline: (timeline: TimelineSettings) => invoke<void>("set_timeline", { timeline }),
  /** For each list of terms, the [id, time] of the VOD's chat messages with any of them as a whole word or phrase. */
  chatMatches: (vodId: number, terms: string[][]) => invoke<[number, number][][]>("chat_matches", { vodId, terms }),
  /** Fetches a VOD's chat again after it failed. */
  retryChat: (id: number) => invoke<void>("retry_chat", { id }),
  updateYtDlp: () => invoke<string>("update_yt_dlp"),
  kickStream: (url: string) => invoke<{ name: string; height: number; url: string }[]>("kick_stream", { url }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  gpuStatus: () => invoke<GpuStatus>("gpu_status"),
  installGpuPack: () => invoke<void>("install_gpu_pack"),
  /** Stops the GPU add-on's download (its pack starts over next time); `gpuDone` then fires with null. */
  cancelGpuInstall: () => invoke<void>("cancel_gpu_install"),
  removeGpuPack: () => invoke<void>("remove_gpu_pack"),
  cacheSize: () => invoke<number>("cache_size"),
  /** Bytes of the videos kept from sites without embeds. */
  mediaSize: () => invoke<number>("media_size"),
  setPlayable: (playable: Playable) => invoke<void>("set_playable", { playable }),
  /** Deletes the video kept for VOD `id`, keeping its transcript. */
  deleteVideo: (id: number) => invoke<void>("delete_video", { id }),
  clearCache: () => invoke<void>("clear_cache"),
};

export const events = {
  vod: (cb: (vod: Vod) => void) => listen<Vod>("vod", (e) => cb(e.payload)),
  vodProgress: (cb: (id: number, status: Status, progress: number) => void) =>
    listen<[number, Status, number]>("vod-progress", (e) => cb(...e.payload)),
  vodRemoved: (cb: (id: number) => void) => listen<number>("vod-removed", (e) => cb(e.payload)),
  /** Which setup item ("yt-dlp", "ffmpeg", "model", "deno") and where its download is. */
  setupProgress: (cb: (item: string, progress: DownloadProgress) => void) =>
    listen<DownloadProgress & { item: string }>("setup-progress", (e) => cb(e.payload.item, e.payload)),
  setupDone: (cb: (error: string | null) => void) => listen<string | null>("setup-done", (e) => cb(e.payload)),
  /** Where the GPU add-on's download is, counted across the pack and its model. */
  gpuProgress: (cb: (progress: DownloadProgress) => void) =>
    listen<DownloadProgress>("gpu-progress", (e) => cb(e.payload)),
  gpuDone: (cb: (error: string | null) => void) => listen<string | null>("gpu-done", (e) => cb(e.payload)),
  ytDlpProgress: (cb: (progress: DownloadProgress) => void) =>
    listen<DownloadProgress>("yt-dlp-progress", (e) => cb(e.payload)),
  /** How far a VOD's chat fetch is, 0..1. */
  chatProgress: (cb: (id: number, fraction: number) => void) =>
    listen<[number, number]>("chat-progress", (e) => cb(...e.payload)),
  /** A batch of a VOD's chat was saved while it's being fetched; `chat` has it now. */
  chatSaved: (cb: (id: number) => void) => listen<number>("chat-saved", (e) => cb(e.payload)),
  clip: (cb: (clip: Clip) => void) => listen<Clip>("clip", (e) => cb(e.payload)),
  /** A VOD's video download: `progress` null when waiting or over, `error` if it failed. */
  videoProgress: (cb: (p: { id: number; progress: number | null; error: string | null }) => void) =>
    listen<{ id: number; progress: number | null; error: string | null }>("video-progress", (e) => cb(e.payload)),
  clipProgress: (cb: (id: number, progress: number) => void) =>
    listen<{ id: number; progress: number }>("clip-progress", (e) => cb(e.payload.id, e.payload.progress)),
  clipRemoved: (cb: (id: number) => void) => listen<number>("clip-removed", (e) => cb(e.payload)),
};

export type { UnlistenFn };

const thumbUrls = new Map<string, Promise<string>>();

/**
 * A blob URL for a VOD's local thumbnail. The bytes come over IPC rather than a
 * custom URL scheme, which WebKitGTK won't load into the dev server's http page.
 */
export function thumbUrl(name: string): Promise<string> {
  let url = thumbUrls.get(name);
  if (!url) {
    const type = { jpg: "image/jpeg", png: "image/png", webp: "image/webp", gif: "image/gif" }[
      name.split(".").pop() ?? ""
    ];
    url = invoke<ArrayBuffer>("thumbnail", { name }).then((bytes) => URL.createObjectURL(new Blob([bytes], { type })));
    url.catch(() => thumbUrls.delete(name));
    thumbUrls.set(name, url);
  }
  return url;
}

const emoteUrls = new Map<string, Promise<string>>();
/** The blob URLs already made, for showing an emote at once. */
const emoteUrlsReady = new Map<string, string>();

/** The blob URL for emote `url` if it's been fetched before. */
export const emoteUrlNow = (url: string): string | undefined => emoteUrlsReady.get(url);

/**
 * A blob URL for a chat emote, by its URL on the emote provider's CDN. The app
 * fetches each emote once and keeps it, so chat doesn't load images from the
 * CDNs directly; the browser sniffs the image type.
 */
export function emoteUrl(url: string): Promise<string> {
  let blob = emoteUrls.get(url);
  if (!blob) {
    blob = invoke<ArrayBuffer>("emote", { url }).then((bytes) => {
      const made = URL.createObjectURL(new Blob([bytes]));
      emoteUrlsReady.set(url, made);
      return made;
    });
    blob.catch(() => emoteUrls.delete(url));
    emoteUrls.set(url, blob);
  }
  return blob;
}
