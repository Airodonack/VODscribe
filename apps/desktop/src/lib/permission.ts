// The note shown before downloading a video or a clip from a site, until its
// "Don't show this again" is ticked (remembered on this computer only).

const HIDDEN = "vodscribe.clipNoteHidden";

export const PERMISSION_NOTE =
  "Only download videos and clips you have permission to use, such as from your own channel or with the streamer's okay.";
export const OWN_VODS_NOTE = "Streamers can also download their whole VODs from their channel's video manager.";

export function noteHidden(): boolean {
  try {
    return localStorage.getItem(HIDDEN) === "true";
  } catch {
    return false;
  }
}

export function hideNote() {
  try {
    localStorage.setItem(HIDDEN, "true");
  } catch {
    // Shown again next time.
  }
}
