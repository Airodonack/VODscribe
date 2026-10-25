// What a cast member is called on screen. A character played by a streamer
// reads as the character first, the streamer after: "Gigi (GEEGA)". A
// streamer whose entry is their own name, or a cast without roster data, is
// their name alone. Pure, so Node tests it (castNames.test.mjs).

import type { CastMember } from "./api";

/** Whether two names are one, case and spacing at the ends aside (as annotated.ts `castIndex` matches a speaker). */
export const sameName = (a: string, b: string): boolean => a.trim().toLowerCase() === b.trim().toLowerCase();

/** A cast member as the views name them: "<name> (<player>)" where a streamer plays them under another name, else their name. */
export function displayName(member: Pick<CastMember, "name" | "player">): string {
  const player = member.player?.trim();
  return player && !sameName(player, member.name) ? `${member.name} (${player})` : member.name;
}

/** A line's speaker as the views name them: the cast member's display name, or `said` as it is for someone not in the cast. */
export const speakerName = (member: Pick<CastMember, "name" | "player"> | undefined, said: string): string =>
  member ? displayName(member) : said;
