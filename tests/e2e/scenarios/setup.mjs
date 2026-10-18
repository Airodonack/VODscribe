// First run: setup (if the tools and model aren't there yet), then the library.

export default {
  name: "setup",
  async run(app, ctx) {
    await app.toLibrary();
    await ctx.checkpoint("library-empty");
    // The workflow keeps test runs out of the update check-ins (latest.json).
    if (process.env.VODSCRIBE_NO_UPDATE_CHECK && (await app.invoke("self_updatable")) !== false) {
      throw new Error("VODSCRIBE_NO_UPDATE_CHECK didn't reach the app: it would check for updates");
    }
  },
};
