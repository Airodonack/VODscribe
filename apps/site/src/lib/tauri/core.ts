// Stands in for @tauri-apps/api/core in the demo (vite.config.ts). Commands
// the demo can answer go to the backend; the rest only work in the app, and
// their controls are grayed out (apps/desktop/src/lib/demo.ts).

import * as backend from "./backend";
import type { SearchFilter } from "desktop/lib/api.ts";

type Args = Record<string, unknown>;

const layer = (id: string, name: string, color: string, terms: string[], visible: boolean) => ({
  id,
  name,
  color,
  terms: terms.map((text) => ({ text, emote: null })),
  visible,
  channel: null,
});
let timeline = {
  layers: [
    layer("laughter", "Laughter", "#f59e0b", ["KEKW", "LUL", "OMEGALUL", "LMAO", "LOL", "ICANT"], true),
    layer("hype", "Hype", "#ec4899", ["Pog", "PogChamp", "POGGERS", "PogU", "W"], false),
    layer("clip", "Clip it", "#ef4444", ["clip", "clipped", "clip it"], false),
  ],
  events: [] as string[],
};

let marks: { id: number; vod_id: number; kind: string; start: number; text: string | null }[] = [];
let lastMark = 0;

const commands: Record<string, (args: Args) => unknown> = {
  setup_status: () => ({
    yt_dlp: true,
    yt_dlp_managed: true,
    ffmpeg: true,
    ffmpeg_managed: true,
    model: true,
    js_runtime: true,
    js_runtime_managed: true,
    can_install_tools: true,
    // model::CPU_MODEL.total_bytes() in crates/core; setup isn't shown in the demo.
    model_bytes: 949_807_475,
    running: false,
  }),
  list_vods: () => backend.listVods(),
  library_order: async () => (await backend.listVods()).map((v) => ({ kind: "vod", id: v.id })),
  reorder_library: ({ items }) =>
    backend.reorder((items as { kind: string; id: number }[]).filter((i) => i.kind === "vod").map((i) => i.id)),
  search: ({ query, filter }) => backend.search(query as string, (filter ?? {}) as SearchFilter),
  transcript: ({ vodId, from, to }) =>
    backend.transcript(vodId as number, from as number | undefined, to as number | null | undefined),
  transcript_stretches: ({ vodId }) => backend.transcriptStretches(vodId as number),
  thumbnail: async ({ name }) => (await fetch(`/demo/thumbs/${encodeURIComponent(name as string)}`)).arrayBuffer(),
  get_settings: () => ({ device: "cpu" }),
  self_updatable: () => true,
  chat_overview: ({ vodId, duration, bins }) =>
    backend.chatOverview(vodId as number, duration as number, bins as number),
  chat_windows: ({ vodId, ranges, cap }) =>
    backend.chatWindows(vodId as number, ranges as [number, number][], cap as number),
  chat_emotes: ({ vodId }) => backend.chatEmotes(vodId as number),
  // 7TV, BTTV and FFZ emotes: none here, as the demo shows emotes by name.
  chat_word_emotes: () => [],
  chat_matches: ({ vodId, terms }) => backend.chatMatches(vodId as number, terms as string[][]),
  // The layers a new library starts with (crates/core layers.rs `defaults`), changed for this visit only.
  get_timeline: () => timeline,
  set_timeline: ({ timeline: next }) => void (timeline = next as typeof timeline),
  // Clips are made in the app only; marks on the timeline last this visit.
  list_clips: () => [],
  list_marks: ({ vodId }) => marks.filter((m) => m.vod_id === vodId),
  add_mark: ({ vodId, time, kind, text }) => {
    const mark = {
      id: ++lastMark,
      vod_id: vodId as number,
      kind: (kind as string | undefined) ?? "cut",
      start: time as number,
      text: (text as string | null | undefined) ?? null,
    };
    marks.push(mark);
    return mark;
  },
  move_mark: ({ id, time }) => {
    const mark = marks.find((m) => m.id === id);
    if (mark) mark.start = time as number;
  },
  set_mark_text: ({ id, text }) => {
    const mark = marks.find((m) => m.id === id);
    if (mark) mark.text = text as string | null;
  },
  delete_mark: ({ id }) => void (marks = marks.filter((m) => m.id !== id)),
  cancel_mark: ({ id }) => void (marks = marks.filter((m) => m.id !== id)),
  // Edits are made in the app only.
  list_edits: () => [],
  // Waveforms made from each VOD's audio (scripts/export-demo.py --peaks);
  // none where there's no file.
  peaks: async ({ id }) => {
    const res = await fetch(`/demo/${id}.peaks`);
    // (A missing file can come back as the site's page.)
    const html = res.headers.get("content-type")?.startsWith("text/html");
    return res.ok && !html ? res.arrayBuffer() : new ArrayBuffer(0);
  },
};

export async function invoke<T>(cmd: string, args: Args = {}): Promise<T> {
  const command = commands[cmd];
  if (!command) throw "That only works in the app. Download VODscribe to try it.";
  return (await command(args)) as T;
}
