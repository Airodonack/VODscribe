// The note shown before downloading a video or a clip from a site, until its
// "Don't show this again" is ticked (remembered on this computer only).

import { load, save } from "./stored";

const HIDDEN = "vodscribe.clipNoteHidden";

export const PERMISSION_NOTE =
  "Only download videos and clips you have permission to use, such as from your own channel or with the streamer's okay.";
export const OWN_VODS_NOTE = "Streamers can also download their whole VODs from their channel's video manager.";

export const noteHidden = () => load(HIDDEN) === "true";
export const hideNote = () => save(HIDDEN, "true");
