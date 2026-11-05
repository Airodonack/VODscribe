// Seeks that land. On WebKitGTK 2.54, a seek made while a video is still
// loading can be dropped, or end at 0 when it was asked to go elsewhere
// (`seeking` at the time asked, then `seeked` at 0). So the time asked is
// kept until a seek ends there, asked again when the metadata is in and
// after a seek that ends somewhere else (a few times), and given as the
// playhead meanwhile. On macOS one asked before the first data is in can
// end "there", then go back to 0 as the data comes in, with no seek of its
// own: so a seek that ends before then isn't taken as landed, and the time
// is asked again once the data is in.

/** How many times a seek is asked again before it's left where it is. */
const TRIES = 3;
/** HTMLMediaElement.HAVE_CURRENT_DATA: the data at the playhead is in. */
const HAVE_CURRENT_DATA = 2;

export function landingSeeks(video: HTMLMediaElement) {
  let wanted: number | null = null;
  let tries = 0;
  const there = () => wanted !== null && Math.abs(video.currentTime - wanted) < 0.05;
  // Once the metadata is in, asked again whatever the video says: it may
  // say it's there and not be.
  video.addEventListener("loadedmetadata", () => {
    if (wanted !== null) video.currentTime = wanted;
  });
  video.addEventListener("loadeddata", () => {
    if (wanted === null) return;
    // There with no seek still under way: landed (its `seeked` came before).
    if (!there()) video.currentTime = wanted;
    else if (!video.seeking) wanted = null;
  });
  video.addEventListener("seeked", () => {
    if (wanted === null || video.readyState < HAVE_CURRENT_DATA) return;
    if (there() || tries >= TRIES) wanted = null;
    else {
      tries++;
      video.currentTime = wanted;
    }
  });
  return {
    seek(seconds: number) {
      wanted = seconds;
      tries = 0;
      video.currentTime = seconds;
    },
    /** Where it's going, while a seek hasn't landed there yet. */
    wanted: () => wanted,
  };
}
