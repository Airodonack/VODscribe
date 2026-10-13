// Stands in for @tauri-apps/api/core in the demo (vite.config.ts). Commands
// the demo can answer go to the backend; the rest only work in the app, and
// their controls are grayed out (apps/desktop/src/lib/demo.ts).

import * as backend from "./backend";

type Args = Record<string, unknown>;

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
  reorder_vods: ({ ids }) => backend.reorder(ids as number[]),
  search: ({ query, vodId }) => backend.search(query as string, (vodId as number | null) ?? null),
  transcript: ({ vodId }) => backend.transcript(vodId as number),
  thumbnail: async ({ name }) => (await fetch(`/demo/thumbs/${encodeURIComponent(name as string)}`)).arrayBuffer(),
  get_settings: () => ({ device: "cpu" }),
  self_updatable: () => true,
};

export async function invoke<T>(cmd: string, args: Args = {}): Promise<T> {
  const command = commands[cmd];
  if (!command) throw "That only works in the app. Download VODscribe to try it.";
  return (await command(args)) as T;
}
