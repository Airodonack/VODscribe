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

/** The app's own player's time, whether it's playing, whether the time
 *  moves yet (playing, it stands until the picture and sound start), how
 *  fast it plays, and where the frame on screen is (null: none shows),
 *  which the playhead follows (player.rs). */
export interface PlayerStatus {
  /** Counts up with each status the app makes (none on the page's own guesses): older ones are stale. */
  seq?: number;
  time: number;
  playing: boolean;
  running: boolean;
  speed: number;
  shown: number | null;
}

/** What the app's own player says of what it opened: the frame rate at the start, if known, and the length. */
/** A stretch of an edit as the app's own player plays it (player.rs `player_plan`). */
export interface PlayerStretch {
  at: number;
  length: number;
  piece_id: number | null;
  /** Its footage is here; else it plays nothing until it is. */
  footage: boolean;
}

export interface PlayerOpened {
  fps: number | null;
  duration: number;
}

export type ThemeChoice = "system" | "light" | "dark";

export interface Settings {
  device: Device;
  theme: ThemeChoice;
  /** Where chat's word emotes come from, in priority order (crates/core `settings::EmoteProviders`). */
  emote_providers: EmoteProvider[];
  /** The tallest video kept from sites without embeds; unset means 720. */
  video_height: number | null;
  /** What clips and rendered edits are made as (crates/core `settings::ClipFormat`). */
  clip_format: ClipFormat;
}

/** MP4 (H.264, AAC), or MOV (DNxHR, PCM) for DaVinci Resolve on Linux. */
export type ClipFormat = "mp4" | "mov";

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
  /** In the clip bar (saved there), not only dragged out of the window. */
  shown: boolean;
}

/** A video made of pieces of VODs (crates/core `edits::Edit`). */
export interface Edit {
  id: number;
  title: string;
  /** Its rendered video's: waiting (not rendered), cutting (rendering), ready or error. */
  status: ClipStatus;
  file_path: string | null;
  error: string | null;
  /** Whether the rendered video is of the edit as it is now. */
  current: boolean;
  pieces: number;
  /** How long it plays, in seconds. */
  length: number;
  /** The VODs its pieces are of. */
  vod_ids: number[];
  /** What it plays of each VOD, in order: its pieces less what's struck. */
  ranges: { vod_id: number; start: number; end: number }[];
}

/** A piece of an edit: `start`..`end` of its VOD, less what's `struck`; its
 *  own, copied from a clip or the timeline (crates/core `edits::EditPiece`). */
export interface EditPiece {
  id: number;
  edit_id: number;
  vod_id: number;
  start: number;
  end: number;
  title: string;
  struck: [number, number][];
  vod_title: string;
}

/** A section (`from`..`to` of VOD `vod_id`) downloading for edits: its
 *  footage can be used up to `usable` (the VOD's time) already; before that's
 *  known, `progress` (0..1) guesses how far it is. */
export interface Fetching {
  vod_id: number;
  from: number;
  to: number;
  progress: number;
  usable: number | null;
}

export interface EditView {
  edit: Edit;
  pieces: EditPiece[];
}

/** A row of the library's list (crates/core `edits::LibraryItem`): VODs and edits share one order. */
export type LibraryItem = { kind: "vod" | "edit"; id: number };

export type MarkKind = "cut" | "chapter" | "highlight";

/** A mark on a VOD's timeline (crates/core `db::Mark`): a cut between two
 *  frames, a chapter's start (it runs to the next), or a highlight. */
export interface Mark {
  id: number;
  vod_id: number;
  kind: MarkKind;
  start: number;
  /** A chapter's title, or a highlight's note. */
  text: string | null;
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

/** A document with an undo history (crates' history.rs): an edit, or a VOD's marks and clip bar. */
export type Doc = { kind: "edit" | "vod"; id: number };

/** What an undo or redo did: to which document, and what it was ("Piece removed"). */
export interface Undone {
  doc: Doc;
  label: string;
}

export const api = {
  /** Why the library didn't open when the app started (recovery.rs); null if it did. */
  libraryProblem: () => invoke<string | null>("library_problem"),
  /** Sets the library that won't open aside, so a new one is made on restart; returns its new name. */
  setLibraryAside: () => invoke<string>("set_library_aside"),
  /** Shows the library's file in the file manager. */
  showLibrary: () => invoke<void>("show_library"),
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
  addMark: (vodId: number, time: number, kind: MarkKind = "cut", text: string | null = null) =>
    invoke<Mark>("add_mark", { vodId, time, kind, text }),
  setMarkText: (id: number, text: string | null) => invoke<void>("set_mark_text", { id, text }),
  /** Whether DaVinci Resolve is installed on Linux, where it can't read MP4 clips. */
  resolveInstalled: () => invoke<boolean>("resolve_installed"),
  /** The view open (an edit or a VOD's), for the history: what changes are made from. */
  setView: (view: Doc | null) => invoke<void>("set_view", { view }),
  /** Undoes the newest change in `view`'s history (or one made from it); null if there's none. */
  undo: (view: Doc) => invoke<Undone | null>("undo", { view }),
  /** Redoes what was undone last in `view`'s history. */
  redo: (view: Doc) => invoke<Undone | null>("redo", { view }),
  /** Whether the app's own player can play here (a native screen in the window). */
  playerAvailable: () => invoke<boolean>("player_available"),
  /** Opens VOD `id`'s file in the app's own player at `at` seconds, playing if `play`. */
  playerOpen: (id: number, at: number, play: boolean) => invoke<PlayerOpened>("player_open", { id, at, play }),
  /** Opens edit `id` in the app's own player, its pieces end to end, at `at` seconds. */
  playerOpenEdit: (id: number, at: number, play: boolean) => invoke<PlayerOpened>("player_open_edit", { id, at, play }),
  /** Edit `id`'s stretches as its pieces and footage are now; the player open for it plays them from here on. */
  playerPlan: (id: number) => invoke<PlayerStretch[]>("player_plan", { id }),
  playerClose: () => invoke<void>("player_close"),
  playerPlay: () => invoke<PlayerStatus>("player_play"),
  playerPause: () => invoke<PlayerStatus>("player_pause"),
  playerSeek: (time: number, play: boolean) => invoke<PlayerStatus>("player_seek", { time, play }),
  playerStep: (frames: number) => invoke<PlayerStatus>("player_step", { frames }),
  /** Plays `speed` times as fast, the sound at the same pitch. */
  playerSetSpeed: (speed: number) => invoke<PlayerStatus>("player_set_speed", { speed }),
  playerSetVolume: (volume: number) => invoke<void>("player_set_volume", { volume }),
  /** Where the page's player box is (Screen.svelte); none when no player is open. */
  /** What's drawn over the screen's picture (screenOverlay.ts): RGBA pixels as a canvas gives
   *  them, `size` of them, shown in `rect` (CSS pixels in the player box); none takes it away. */
  setScreenOverlay: (
    overlay: {
      rect: { x: number; y: number; width: number; height: number };
      size: [number, number];
      pixels: Uint8ClampedArray;
    } | null,
  ) =>
    overlay
      ? invoke<void>("set_screen_overlay", new Uint8Array(overlay.pixels.buffer), {
          headers: { overlay: JSON.stringify({ rect: overlay.rect, size: overlay.size }) },
        })
      : invoke<void>("set_screen_overlay", new Uint8Array(0)),
  setScreen: (rect: { x: number; y: number; width: number; height: number } | null) =>
    invoke<void>("set_screen", { rect }),
  listEdits: () => invoke<Edit[]>("list_edits"),
  createEdit: () => invoke<Edit>("create_edit"),
  getEdit: (id: number) => invoke<EditView>("get_edit", { id }),
  renameEdit: (id: number, title: string) => invoke<EditView>("rename_edit", { id, title }),
  deleteEdit: (id: number) => invoke<void>("delete_edit", { id }),
  /** Puts a copy of clip `clipId`'s stretch in edit `editId`. */
  addToEdit: (editId: number, clipId: number, index?: number) =>
    invoke<EditView>("add_to_edit", { editId, clipId, index }),
  /** Puts `start`..`end` of VOD `vodId` in edit `editId`, named `title` (else for what's said). */
  addRangeToEdit: (editId: number, vodId: number, start: number, end: number, title?: string, index?: number) =>
    invoke<EditView>("add_range_to_edit", { editId, vodId, start, end, title, index }),
  /** Edit `id`'s picture (its first frame, or its VOD's thumbnail); empty if none. */
  editThumb: (id: number) => invoke<ArrayBuffer>("edit_thumb", { id }).then((b) => new Uint8Array(b)),
  /** VOD `vodId`'s frame at `at` seconds, a JPEG `width` wide, from its footage here; empty if none of that moment is. */
  vodFrame: (vodId: number, at: number, width: number) =>
    invoke<ArrayBuffer>("vod_frame", { vodId, at, width }).then((b) => new Uint8Array(b)),
  movePiece: (id: number, index: number) => invoke<EditView>("move_piece", { id, index }),
  trimPiece: (id: number, start: number, end: number) => invoke<EditView>("trim_piece", { id, start, end }),
  strikePiece: (id: number, struck: [number, number][]) => invoke<EditView>("strike_piece", { id, struck }),
  removePiece: (id: number) => invoke<EditView>("remove_piece", { id }),
  splitPiece: (id: number, at: number) => invoke<EditView>("split_piece", { id, at }),
  renderEdit: (id: number) => invoke<void>("render_edit_now", { id }),
  stopRender: (id: number) => invoke<void>("stop_render", { id }),
  showEdit: (id: number) => invoke<void>("show_edit", { id }),
  dragEdit: (id: number) => invoke<void>("drag_edit", { id }),
  moveMark: (id: number, time: number) => invoke<void>("move_mark", { id, time }),
  deleteMark: (id: number) => invoke<void>("delete_mark", { id }),
  /** Takes away a mark just added, as if never made (not undoable, nor its adding). */
  cancelMark: (id: number) => invoke<void>("cancel_mark", { id }),
  addClip: (vodId: number, start: number, end: number, shown = true) =>
    invoke<Clip>("add_clip", { vodId, start, end, shown }),
  /** Puts clip `id` (one only dragged out) in the clip bar. */
  keepClip: (id: number) => invoke<Clip>("keep_clip", { id }),
  setClipRange: (id: number, start: number, end: number) => invoke<Clip>("set_clip_range", { id, start, end }),
  renameClip: (id: number, title: string) => invoke<Clip>("rename_clip", { id, title }),
  deleteClip: (id: number) => invoke<void>("delete_clip", { id }),
  showClip: (id: number) => invoke<void>("show_clip", { id }),
  /** Starts dragging clip `id`'s file out of the window; call while the mouse button is down. */
  dragClip: (id: number, image?: Uint8Array) =>
    invoke<void>("drag_clip", { id, image: image ? Array.from(image) : null }),
  /** VOD `id`'s frame at `time`, a small JPEG; empty if it doesn't play from a file here. */
  frame: (id: number, time: number) => invoke<ArrayBuffer>("frame", { id, time }).then((b) => new Uint8Array(b)),
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
  libraryOrder: () => invoke<LibraryItem[]>("library_order"),
  reorderLibrary: (items: LibraryItem[]) => invoke<void>("reorder_library", { items }),
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
  /** The app's own player's time and state, at every change and now and then while playing. */
  playerTime: (cb: (status: PlayerStatus) => void) => listen<PlayerStatus>("player-time", (e) => cb(e.payload)),
  /** A document put back as it was by undo or redo: what shows it reloads it. */
  restored: (cb: (doc: Doc) => void) => listen<Doc>("restored", (e) => cb(e.payload)),
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
  edit: (cb: (edit: Edit) => void) => listen<Edit>("edit", (e) => cb(e.payload)),
  editProgress: (cb: (id: number, progress: number) => void) =>
    listen<{ id: number; progress: number }>("edit-progress", (e) => cb(e.payload.id, e.payload.progress)),
  editRemoved: (cb: (id: number) => void) => listen<number>("edit-removed", (e) => cb(e.payload)),
  /** The system's drag of a clip's file (out of the window) is over, dropped or not. */
  dragEnded: (cb: () => void) => listen("drag-ended", () => cb()),
  /** How far a section downloading for edits is. */
  footageProgress: (cb: (fetching: Fetching) => void) => listen<Fetching>("footage-progress", (e) => cb(e.payload)),
  /** Footage of VOD `vod_id` downloaded for edits (or, with `error`, not). */
  footage: (cb: (fetched: { vod_id: number; error: string | null }) => void) =>
    listen<{ vod_id: number; error: string | null }>("footage", (e) => cb(e.payload)),
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
