// Where a cast member's name and disc lead (Transcript.svelte's headings,
// TranscriptHead.svelte's Cast list, AnnotationsPane.svelte's chips,
// CastDisc.svelte): their channel; or, when their own stream ran alongside
// this VOD and covers the moment (povs.ts), that stream at the same moment,
// in the app when the library has it (`povLibrary`), else on its site. A
// text-to-speech voice leads nowhere.

import { openUrl } from "@tauri-apps/plugin-opener";
import { channelLabel, channelUrl } from "./accounts";
import { annotated } from "./annotated.svelte";
import { isVoice } from "./annotated";
import type { CastMember, Vod } from "./api";
import { isVideo, povLink, povTitle, type PovLink } from "./povs";

/**
 * The library's VODs, and how one is opened at a moment: App.svelte's,
 * while it has a library (the website's VOD page has none, so its links go
 * to the sites).
 */
class PovLibrary {
  vods = $state.raw<Vod[]>([]);
  open = $state.raw<((vodId: number, time: number) => void) | null>(null);
}

export const povLibrary = new PovLibrary();

/** A cast member's link: its address, what its tooltip says, how a click
 *  follows it, the stream it goes to (null: their channel), and whether
 *  following it leaves the app for the browser (`outside`; false only for a
 *  stream the library has, opened here), which its name shows with the
 *  external-link sign. */
export type CastLink = { href: string; title: string; follow: () => void; pov: PovLink | null; outside: boolean };

/**
 * Where `member`'s link goes at `moment` seconds into `vod` (null: no
 * moment, their channel); null when it goes nowhere (no account, or a
 * text-to-speech voice).
 */
export function castLink(
  member: CastMember | undefined,
  vod: Vod | null | undefined,
  moment: number | null,
): CastLink | null {
  const account = member?.account;
  if (!account || isVoice(member)) return null;
  const povs = vod ? (annotated.of(vod.id)?.povs ?? []) : [];
  const pov = vod ? povLink(povs, account, vod.started_at, moment) : null;
  if (!pov) {
    const url = channelUrl(account);
    return { href: url, title: channelLabel(account), follow: () => void openUrl(url), pov: null, outside: true };
  }
  const open = povLibrary.open;
  const here = open ? povLibrary.vods.find((v) => v.listed && isVideo(v, pov.pov.video)) : undefined;
  if (open && here)
    return {
      href: pov.url,
      title: povTitle(pov.pov, pov.offset, true),
      follow: () => open(here.id, pov.offset),
      pov,
      outside: false,
    };
  return { href: pov.url, title: pov.title, follow: () => void openUrl(pov.url), pov, outside: true };
}
