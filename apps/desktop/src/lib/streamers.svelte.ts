// The streamer card that's open (StreamerCard.svelte, one for the whole
// page, in App.svelte), and the cards as the file host gives them, kept for
// the session (streamers.ts `CardCache`). A click on a person's name or
// picture opens theirs at the pointer (`openCard`): the transcript's speaker
// headings, the Cast list, the highlights' chips, the VOD's channel over the
// player.

import { annotated } from "./annotated.svelte";
import { channelAccount, channelPage, channelPerson, speakerColor } from "./annotated";
import { api, type CastMember, type Vod } from "./api";
import { displayName } from "./castNames";
import { CardCache } from "./streamers";

export const cards = new CardCache(
  (id) => api.streamerCard(id),
  () => api.streamerIndex(),
);

/** Whose card is open, and from where. */
export type CardFor = {
  member: CastMember;
  /** What the views call them ("Gigi (GEEGA)"): the card's heading until their card says better. */
  name: string;
  /** Their colour in the VOD's cast, for the picture's initial; null when they aren't in it. */
  color: string | null;
  vod: Vod | null;
  /** The moment it's opened at, in seconds into the VOD (a heading's lines' start, the playhead), for a link to their own stream then. */
  moment: number | null;
  /** Their account to look their card up by when the cast names none: the VOD's channel's. */
  account?: CastMember["account"];
  /** The VOD's channel's page, from the VOD itself, for a site other than Twitch and Kick (annotated.ts `channelPage`): one of the card's links. */
  page?: { label: string; href: string };
};

class OpenCard {
  open = $state(false);
  of = $state.raw<CardFor | null>(null);
  /** Where the pointer was, in the window. */
  at = $state.raw<{ x: number; y: number }>({ x: 0, y: 0 });
  /** What was clicked: a click on it again doesn't close the card as a click outside it. */
  anchor = $state.raw<HTMLElement | undefined>(undefined);
}

export const openCard = new OpenCard();

/**
 * Opens `of`'s card where `e` was clicked: at the pointer, or for a click
 * from the keyboard (no pointer), under what was clicked.
 */
export function showCard(e: MouseEvent, of: CardFor) {
  e.preventDefault();
  const el = e.currentTarget instanceof HTMLElement ? e.currentTarget : undefined;
  const box = el?.getBoundingClientRect();
  const keyboard = e.detail === 0 && box;
  openCard.at = keyboard ? { x: box.left, y: box.bottom } : { x: e.clientX, y: e.clientY };
  openCard.anchor = el;
  openCard.of = of;
  openCard.open = true;
}

/**
 * Whose card the VOD's channel opens (its name over the player): the person
 * the channel is (annotated.ts `channelPerson`, the one whose picture is
 * beside the name), looked up by the channel's own account where the VOD has
 * one (`channelAccount`), its page among the card's links where the VOD
 * knows it (`channelPage`).
 */
export function channelCard(vod: Vod): CardFor {
  const cast = annotated.of(vod.id)?.cast ?? [];
  const account = channelAccount(vod);
  const member = channelPerson(vod, cast) ?? { name: "", aliases: [], role: "streamer" };
  const page = channelPage(vod);
  return {
    member,
    name: displayName(member),
    color: speakerColor(cast, member.name),
    vod,
    moment: null,
    account,
    ...(page ? { page } : {}),
  };
}
