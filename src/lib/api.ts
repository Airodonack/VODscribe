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
  published: string | null;
  status: Status;
  progress: number;
  error: string | null;
  added_at: number;
  /** Not an error: e.g. which device transcribed it, or why the GPU wasn't used. */
  note: string | null;
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
}

export interface SearchHit {
  vod_id: number;
  segment_id: number;
  time: number;
  parts: { text: string; hit: boolean }[];
}

export interface SetupStatus {
  yt_dlp: boolean;
  ffmpeg: boolean;
  model: boolean;
  js_runtime: boolean;
  running: boolean;
}

export interface Line {
  start: number;
  text: string;
}

export const api = {
  setupStatus: () => invoke<SetupStatus>("setup_status"),
  runSetup: () => invoke<void>("run_setup"),
  addVod: (url: string) => invoke<{ vod: Vod; created: boolean }>("add_vod", { url }),
  listVods: () => invoke<Vod[]>("list_vods"),
  deleteVod: (id: number) => invoke<void>("delete_vod", { id }),
  retryVod: (id: number) => invoke<void>("retry_vod", { id }),
  cancelVod: (id: number) => invoke<void>("cancel_vod", { id }),
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
  removeGpuPack: () => invoke<void>("remove_gpu_pack"),
};

export const events = {
  vod: (cb: (vod: Vod) => void) => listen<Vod>("vod", (e) => cb(e.payload)),
  vodProgress: (cb: (id: number, status: Status, progress: number) => void) =>
    listen<[number, Status, number]>("vod-progress", (e) => cb(...e.payload)),
  vodRemoved: (cb: (id: number) => void) => listen<number>("vod-removed", (e) => cb(e.payload)),
  setupProgress: (cb: (step: string, done: number, total: number | null) => void) =>
    listen<[string, number, number | null]>("setup-progress", (e) => cb(...e.payload)),
  setupDone: (cb: (error: string | null) => void) => listen<string | null>("setup-done", (e) => cb(e.payload)),
  gpuProgress: (cb: (step: string, done: number, total: number) => void) =>
    listen<[string, number, number]>("gpu-progress", (e) => cb(...e.payload)),
  gpuDone: (cb: (error: string | null) => void) => listen<string | null>("gpu-done", (e) => cb(e.payload)),
};

export type { UnlistenFn };
