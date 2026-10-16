// Seeks that land. On WebKitGTK 2.54, a seek made while a video is still
// loading can be dropped, or end at 0 when it was asked to go elsewhere
// (`seeking` at the time asked, then `seeked` at 0). So the time asked is
// kept until a seek ends there, asked again when the metadata is in and
// after a seek that ends somewhere else (a few times), and given as the
// playhead meanwhile.

/** How many times a seek is asked again before it's left where it is. */
const TRIES = 3;

export function landingSeeks(video: HTMLMediaElement) {
  let wanted: number | null = null;
  let tries = 0;
  // Once the metadata is in, asked again whatever the video says: it may
  // say it's there and not be.
  video.addEventListener("loadedmetadata", () => {
    if (wanted !== null) video.currentTime = wanted;
  });
  video.addEventListener("seeked", () => {
    if (wanted === null) return;
    if (Math.abs(video.currentTime - wanted) < 0.05 || tries >= TRIES) wanted = null;
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
