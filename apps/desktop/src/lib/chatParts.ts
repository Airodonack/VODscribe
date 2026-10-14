// A chat message as it's shown (ChatMessage.svelte): its text, with the
// platform's own emotes and the words that are emotes on 7TV, BTTV or FFZ
// (api `WordEmote`) as images. An overlay emote (a hat, a spin) is drawn over
// the emote before it, when there is one, rather than beside it.

import type { ChatPart, WordEmote } from "./api";

export interface Image {
  name: string;
  url: string;
}

export type Piece = { type: "text"; text: string } | ({ type: "emote"; overlays: Image[] } & Image);

/** `parts` as pieces to show, with `words` (by exact name) made emotes. */
export function pieces(parts: ChatPart[], words: Map<string, WordEmote>): Piece[] {
  const out: Piece[] = [];
  const text = (t: string) => {
    const last = out.at(-1);
    if (last?.type === "text") last.text += t;
    else out.push({ type: "text", text: t });
  };
  for (const part of parts) {
    if (part.type === "emote") {
      out.push({ type: "emote", name: part.name, url: part.url, overlays: [] });
      continue;
    }
    if (words.size === 0) {
      text(part.text);
      continue;
    }
    for (const token of part.text.split(/(\s+)/)) {
      const emote = token ? words.get(token) : undefined;
      if (!emote) {
        if (token) text(token);
        continue;
      }
      // Over the emote before it, if only spaces come between them.
      const last = out.at(-1);
      const before = last?.type === "text" && !last.text.trim() ? out.at(-2) : last;
      if (emote.overlay && before?.type === "emote") {
        if (before !== last) out.pop();
        before.overlays.push({ name: emote.name, url: emote.url });
      } else {
        out.push({ type: "emote", name: emote.name, url: emote.url, overlays: [] });
      }
    }
  }
  return out;
}
