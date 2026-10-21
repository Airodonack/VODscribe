// For headless GNOME Shell test sessions only (tests/e2e/distros.mjs, --env gnome):
// the Shell's Screenshot over D-Bus answers only its own callers, unless in
// unsafe mode, which this turns on, so the tests (lib/os.mjs) can see the
// screen; and it leaves the overview the Shell starts in, which shows
// windows shrunk, as a desktop session does once it's started.
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import { Extension } from "resource:///org/gnome/shell/extensions/extension.js";

export default class VodscribeTests extends Extension {
  enable() {
    global.context.unsafe_mode = true;
    const leave = () => Main.overview.hide();
    if (Main.layoutManager._startingUp) this._started = Main.layoutManager.connect("startup-complete", leave);
    else leave();
    this._shown = Main.overview.connect("shown", leave);
  }

  disable() {
    global.context.unsafe_mode = false;
    if (this._started) Main.layoutManager.disconnect(this._started);
    Main.overview.disconnect(this._shown);
  }
}
