// Reads what opening a VOD reads, before it's opened (the library's row the
// pointer rests on, the rows beside the one open), so it opens with nothing
// to wait for: api.ts keeps each read (kept.ts), and Transcript.svelte and
// cuts.svelte.ts find them there. Its lines and what of it is transcribed,
// its marks and, with its chat fetched, the chat's counts and the first
// rows' messages, asked as the transcript asks for them when it opens at
// the top. (Captions are read once its player opens: not here.)

import { api, type Vod } from "./api";
import { CHAT_BINS, CHAT_SHOWN, buildRows } from "./rows";
import { shownGaps } from "./stretches";

/** The rows whose chat is read ahead: more than a tall window shows at the top. */
const FIRST_ROWS = 40;

export async function readAhead(vod: Vod) {
  void api.listMarks(vod.id).catch(() => {});
  const [lines, stretches] = await Promise.all([api.transcript(vod.id), api.transcriptStretches(vod.id)]);
  if (vod.chat !== "done") return;
  const duration = vod.duration ?? 0;
  // As the transcript asks, before its lines are in.
  void api.chatOverview(vod.id, duration, CHAT_BINS).catch(() => {});
  const gaps = shownGaps(stretches, vod.duration, vod.transcribed, lines.length);
  const rows = buildRows(lines, duration, true, gaps).slice(0, FIRST_ROWS);
  if (rows.length)
    await api.chatWindows(
      vod.id,
      rows.map((r) => [r.from, r.to]),
      CHAT_SHOWN,
    );
}
