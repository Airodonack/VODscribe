// First run: setup (if the tools and model aren't there yet), then the library.
// On Linux, the page has no media session.

export default {
  name: "setup",
  async run(app, ctx) {
    await app.toLibrary();
    await ctx.checkpoint("library-empty");
    // The workflow keeps test runs out of the update check-ins (latest.json).
    if (process.env.VODSCRIBE_NO_UPDATE_CHECK && (await app.invoke("self_updatable")) !== false) {
      throw new Error("VODSCRIBE_NO_UPDATE_CHECK didn't reach the app: it would check for updates");
    }
    // On Linux the page's media session is off (screen.rs media_session_off):
    // WebKitGTK would announce sites' players to the desktop, and Twitch's
    // flood of them hung GNOME Shell. WebKitGTK before 2.42 can't turn it off.
    if (process.platform === "linux" && (await app.js(`return "mediaSession" in navigator`)))
      throw new Error("the page has a media session: WebKit's MediaSession is still on");
  },
};
