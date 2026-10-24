// The Settings window (App.svelte's Modal over Settings.svelte): whether it's
// open, and the section it shows, the one last opened: open again there while
// VODscribe runs, back at the first after a restart. Anything can open it on
// a section of its own (a player's legend, on Controls).

/** The sections, in the list's order. */
export const SECTIONS = [
  { id: "transcription", label: "Transcription", icon: "audio-lines" },
  { id: "appearance", label: "Appearance", icon: "palette" },
  { id: "controls", label: "Controls", icon: "keyboard" },
  { id: "storage", label: "Storage", icon: "folder" },
  { id: "updates", label: "Updates", icon: "download" },
  { id: "privacy", label: "Privacy", icon: "lock" },
  { id: "help", label: "Help", icon: "circle-help" },
] as const;
export type Section = (typeof SECTIONS)[number]["id"];

class SettingsWindow {
  open = $state(false);
  section = $state<Section>(SECTIONS[0].id);

  /** Opens it on `section`. */
  show(section: Section) {
    this.section = section;
    this.open = true;
  }
}

export const settingsWindow = new SettingsWindow();
