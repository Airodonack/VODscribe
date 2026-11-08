// Settings › Encoding's formats, given those the ffmpeg in use can't make
// (the app's `missing_clip_codecs`): each of those is greyed out, says why
// on hover, and can't be picked. One already picked stays picked, since a
// file in another format may not open in the user's editor; its hint says
// clips and edits fail until another is picked, as they then say. Until the app
// has answered (`missing` null), nothing can be picked, as any may be missing.

/** A format Settings offers: its id, its name and its help line. */
export type CodecOption = { id: string; label: string; hint: string };

/** A format as Settings shows it: picked (`on`), greyed out, why on hover, and whether a click picks it. */
export type CodecShown<T> = T & { on: boolean; greyed: boolean; why: string | undefined; pickable: boolean };

/** Why a format is greyed out, on hover. */
export const cantMake = (label: string) => `VODscribe's ffmpeg can't make ${label}.`;

/** The formats `options` as shown with `chosen` picked and `missing` greyed out, and the hint under them. */
export function codecChoices<T extends CodecOption>(
  options: readonly T[],
  chosen: string,
  missing: readonly string[] | null,
): { shown: CodecShown<T>[]; hint: string | undefined } {
  const shown = options.map((o) => {
    const on = o.id === chosen;
    const greyed = !!missing?.includes(o.id);
    // The picked one's hint says why already, under it.
    const why = greyed && !on ? cantMake(o.label) : undefined;
    return { ...o, on, greyed, why, pickable: missing !== null && !greyed };
  });
  const picked = shown.find((o) => o.on);
  const hint = picked?.greyed
    ? `${cantMake(picked.label)} Clips and edits fail until you pick another format.`
    : picked?.hint;
  return { shown, hint };
}
