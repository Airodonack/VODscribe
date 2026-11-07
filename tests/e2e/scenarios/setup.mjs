// First run: setup (if the tools and model aren't there yet), then the library.

export default {
  name: "setup",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "library-empty":
      "The library on a first start, light theme: on the left an empty Paste a VOD link… box with a pale Add button, the line or add video files… or drop files and links anywhere, and No VODs yet; in the middle a film icon over Your library is empty and a short paragraph naming Kick, Twitch and YouTube.",
  },
  async run(app, ctx) {
    await app.toLibrary();
    await ctx.checkpoint("library-empty");
    // The workflow keeps test runs out of the update check-ins (latest.json).
    if (process.env.VODSCRIBE_NO_UPDATE_CHECK && (await app.invoke("self_updatable")) !== false) {
      throw new Error("VODSCRIBE_NO_UPDATE_CHECK didn't reach the app: it would check for updates");
    }
  },
};
