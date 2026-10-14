// The app's colors (Settings → Appearance): the system's light or dark, or
// always one of them. Dark colors are keyed to data-theme on <html> (app.css).
// On Windows and macOS the window's title bar follows too. Not on Linux: the
// desktop theme draws it, and there resetting the window's theme ("system")
// turns off GTK's dark preference, which WebKitGTK's own light or dark (what
// System follows) comes from, so a dark desktop would show light.

import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ThemeChoice } from "./api";
import { DEMO } from "./demo";
import { os } from "./platform";

class Theme {
  /** Whether the app shows its dark colors now, for what draws its own (the minimap). */
  dark = $state(false);
  #choice: ThemeChoice = "system";
  #system = typeof window === "undefined" ? null : window.matchMedia("(prefers-color-scheme: dark)");

  constructor() {
    this.#system?.addEventListener("change", () => this.#update());
    this.#update();
  }

  apply(choice: ThemeChoice) {
    this.#choice = choice;
    const root = document.documentElement;
    if (choice === "system") delete root.dataset.theme;
    else root.dataset.theme = choice;
    // The website's demo has no window of its own to theme.
    if (!DEMO && os !== "linux")
      getCurrentWindow()
        .setTheme(choice === "system" ? null : choice)
        .catch(() => {});
    this.#update();
  }

  #update() {
    this.dark = this.#choice === "dark" || (this.#choice === "system" && !!this.#system?.matches);
  }
}

export const theme = new Theme();
