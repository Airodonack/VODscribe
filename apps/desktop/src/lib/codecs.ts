// Settings › Encoding's formats, given those the ffmpeg in use can't make
// (the app's `missing_clip_codecs`, which says too whether that ffmpeg is
// VODscribe's own or the system's): each of those is greyed out, says why
// on hover, and can't be picked. One already picked stays picked, since a
// file in another format may not open in the user's editor; its hint says
// clips and edits fail until another is picked, as they then say. Until the app
// has answered (`ffmpeg` null), nothing can be picked, as any may be missing.

/** A format Settings offers: its id, its name and its help line. */
export type CodecOption = { id: string; label: string; hint: string };

/** A format as Settings shows it: picked (`on`), greyed out, why on hover, and whether a click picks it. */
export type CodecShown<T> = T & { on: boolean; greyed: boolean; why: string | undefined; pickable: boolean };

/** Why a format is greyed out, on hover, with VODscribe's own ffmpeg in use. */
export const CANT_MAKE_OWN = "VODscribe's ffmpeg can't make {format}.";
/** The same, with the system's ffmpeg in use (none of VODscribe's own). */
export const CANT_MAKE_SYSTEM = "Your system's ffmpeg can't make {format}.";
/** The hint under a picked format that VODscribe's own ffmpeg can't make. */
export const PICKED_CANT_MAKE_OWN =
  "VODscribe's ffmpeg can't make {format}. Clips and edits fail until you pick another format.";
/** The same, with the system's ffmpeg in use. */
export const PICKED_CANT_MAKE_SYSTEM =
  "Your system's ffmpeg can't make {format}. Clips and edits fail until you pick another format.";

/** Why the format named `label` is greyed out, on hover, with VODscribe's own ffmpeg in use (`own`) or the system's. */
export const cantMake = (label: string, own: boolean) =>
  (own ? CANT_MAKE_OWN : CANT_MAKE_SYSTEM).replace("{format}", label);

/** The formats `options` as shown with `chosen` picked and those the ffmpeg in use can't make
 *  (`ffmpeg.missing`, VODscribe's own or the system's: `ffmpeg.own`) greyed out, and the hint under them. */
export function codecChoices<T extends CodecOption>(
  options: readonly T[],
  chosen: string,
  ffmpeg: { missing: readonly string[]; own: boolean } | null,
): { shown: CodecShown<T>[]; hint: string | undefined } {
  const own = ffmpeg?.own ?? true;
  const shown = options.map((o) => {
    const on = o.id === chosen;
    const greyed = !!ffmpeg?.missing.includes(o.id);
    // The picked one's hint says why already, under it.
    const why = greyed && !on ? cantMake(o.label, own) : undefined;
    return { ...o, on, greyed, why, pickable: ffmpeg !== null && !greyed };
  });
  const picked = shown.find((o) => o.on);
  const hint = picked?.greyed
    ? (own ? PICKED_CANT_MAKE_OWN : PICKED_CANT_MAKE_SYSTEM).replace("{format}", picked.label)
    : picked?.hint;
  return { shown, hint };
}
