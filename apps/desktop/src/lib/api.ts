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
  published: string | null;
  status: Status;
  progress: number;
  error: string | null;
  added_at: number;
  /** Not an error: e.g. which device transcribed it, or why the GPU wasn't used. */
  note: string | null;
  /** Has transcript from a model this version no longer uses; unfinished ones start over on resume. */
  outdated: boolean;
}

export type Device = "cpu" | "gpu";

export interface Settings {
  device: Device;
}

export interface GpuStatus {
  supported: boolean;
  installed: boolean;
  installing: boolean;
  worker: boolean;
  gpu: { name: string; total_mb: number; free_mb: number } | null;
  min_free_mb: number;
  /** Bytes the add-on downloads. */
  download_bytes: number;
}

export interface SearchHit {
  vod_id: number;
  segment_id: number;
  time: number;
  parts: { text: string; hit: boolean }[];
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
  text: string;
}

export const api = {
  setupStatus: () => invoke<SetupStatus>("setup_status"),
  runSetup: () => invoke<void>("run_setup"),
  /** Stops setup's downloads; what already finished stays. `setupDone` then fires with null. */
  cancelSetup: () => invoke<void>("cancel_setup"),
  dataDir: () => invoke<string>("data_dir"),
  openDataDir: () => invoke<void>("open_data_dir"),
  addVod: (url: string) => invoke<{ vod: Vod; created: boolean }>("add_vod", { url }),
  addFiles: (paths: string[]) => invoke<{ vods: Vod[]; errors: string[] }>("add_files", { paths }),
  /** The file extensions VODscribe accepts, for the file picker. */
  mediaExtensions: () => invoke<string[]>("media_extensions"),
  showFile: (id: number) => invoke<void>("show_file", { id }),
  openFile: (id: number) => invoke<void>("open_file", { id }),
  mediaUrl: (id: number) => invoke<string>("media_url", { id }),
  listVods: () => invoke<Vod[]>("list_vods"),
  reorderVods: (ids: number[]) => invoke<void>("reorder_vods", { ids }),
  deleteVod: (id: number) => invoke<void>("delete_vod", { id }),
  retryVod: (id: number) => invoke<void>("retry_vod", { id }),
  cancelVod: (id: number) => invoke<void>("cancel_vod", { id }),
  retranscribeVod: (id: number) => invoke<void>("retranscribe_vod", { id }),
  pauseVod: (id: number) => invoke<void>("pause_vod", { id }),
  resumeVod: (id: number) => invoke<void>("resume_vod", { id }),
  search: (query: string, vodId: number | null) =>
    invoke<{ hits: SearchHit[]; truncated: boolean }>("search", { query, vodId }),
  transcript: (vodId: number) => invoke<Line[]>("transcript", { vodId }),
  updateYtDlp: () => invoke<string>("update_yt_dlp"),
  kickStream: (url: string) => invoke<{ name: string; height: number; url: string }[]>("kick_stream", { url }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  gpuStatus: () => invoke<GpuStatus>("gpu_status"),
  installGpuPack: () => invoke<void>("install_gpu_pack"),
  /** Stops the GPU add-on's download; `gpuDone` then fires with null. */
  cancelGpuInstall: () => invoke<void>("cancel_gpu_install"),
  removeGpuPack: () => invoke<void>("remove_gpu_pack"),
  cacheSize: () => invoke<number>("cache_size"),
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
