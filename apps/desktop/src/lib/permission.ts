// The note shown before downloading a video or a clip from a site, until its
// "Don't show this again" is ticked or it's turned off in Settings › Privacy
// (the setting `ask_before_downloading`, asks.ts).

import { settings } from "./appSettings.svelte";
import { shows } from "./asks";

export const PERMISSION_NOTE =
  "Only download videos and clips you have permission to use, such as from your own channel or with the streamer's okay.";
export const OWN_VODS_NOTE = "Streamers can also download their whole VODs from their channel's video manager.";
/** Under the note's "Don't show this again", once ticked. */
export const WHERE_UNDONE = "You can turn it back on in Settings › Privacy.";

export const noteHidden = () => !shows(settings.value, "ask_before_downloading");
export const hideNote = () => void settings.update({ ask_before_downloading: false });
