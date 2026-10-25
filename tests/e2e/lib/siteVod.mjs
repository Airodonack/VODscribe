// VODs "from a site", added by their link without the network: yt-dlp is a
// stand-in in the data folder's bin (the real one put back after) that
// answers any lookup as a YouTube video whose id is the link's last part, a
// minute long (or a Dailymotion one, for a link to /dailymotion/<id>), and
// does nothing else: it has no chat replay, and a download fails. YouTube's,
// as the file host is asked only about Twitch's, YouTube's and Kick's VODs,
// and YouTube's chat comes through yt-dlp alone; the answer names no
// thumbnail and its player loads only on Play, so adding one reaches nothing
// outside this computer. Links are on a name that never resolves
// (.invalid), as the stand-in never looks.

import { chmodSync, existsSync, renameSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "./app.mjs";
import { dataDirs } from "./data.mjs";

/** A yt-dlp answering `-J <link>` as YouTube's (or Dailymotion's) video of the link's last part. */
const STAND_IN = `#!/bin/sh
# The e2e tests' stand-in for yt-dlp (tests/e2e/lib/siteVod.mjs).
json=0; chat=0; link=""
for arg in "$@"; do
  [ "$arg" = "-J" ] && json=1
  [ "$arg" = "--write-subs" ] && chat=1
  link="$arg"
done
# A chat replay asked for: none written, as for a video that wasn't a livestream.
[ "$chat" = 1 ] && exit 0
[ "$json" = 1 ] || exit 1
id="\${link##*/}"
rest="\${link%/*}"
case "\${rest##*/}" in
  dailymotion) printf '{"id":"%s","extractor_key":"Dailymotion","webpage_url":"https://www.dailymotion.com/video/%s","title":"A minute with Marlowe","channel":"Marlowe","duration":60}\\n' "$id" "$id" ;;
  *) printf '{"id":"%s","extractor_key":"Youtube","webpage_url":"https://www.youtube.com/watch?v=%s","title":"A minute with Marlowe","channel":"Marlowe","duration":60}\\n' "$id" "$id" ;;
esac
`;

/** The link the stand-in answers for VOD `id`: YouTube's, or `site`'s ("dailymotion"). */
export const linkTo = (id, site = null) => `https://vods.example.invalid/${site ? `${site}/` : ""}${id}`;

/**
 * Runs `fn(app)` with the stand-in in place: the app is started again with
 * it (and again after, without it, when `fn` is done or fails). Skips where
 * a shell script can't stand in (Windows).
 */
export async function withSiteVods(ctx, fn) {
  if (process.platform === "win32") ctx.skip("the stand-in yt-dlp is a shell script");
  await ctx.quit();
  const bin = path.join(dataDirs().at(-1), "bin");
  const ytDlp = path.join(bin, "yt-dlp");
  const real = path.join(bin, "yt-dlp.e2e-real");
  // Moved aside, not written over: the tools in bin may be links to a seed's.
  if (existsSync(ytDlp)) renameSync(ytDlp, real);
  writeFileSync(ytDlp, STAND_IN);
  chmodSync(ytDlp, 0o755);
  try {
    return await fn(await ctx.start());
  } finally {
    rmSync(ytDlp, { force: true });
    if (existsSync(real)) renameSync(real, ytDlp);
  }
}

/** Adds the stand-in's VOD `id` (YouTube's, or `site`'s) by its link, as pasted in; the VOD, once in the library. */
export async function addSiteVod(app, id, site = null) {
  await app.toLibrary();
  await app.type('input[aria-label="VOD link"]', linkTo(id, site));
  await app.submit('input[aria-label="VOD link"]');
  return until(
    `VOD ${id} added`,
    async () => {
      const error = await app.js(`return document.querySelector(".add-error")?.textContent.trim() ?? ""`);
      if (error) throw new Error(`adding ${id}: ${error}`);
      return (await app.invoke("list_vods")).find((v) => v.platform_id === id);
    },
    MINUTE,
  );
}
