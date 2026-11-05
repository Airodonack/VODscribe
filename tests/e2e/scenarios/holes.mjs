// A hole: an edit with a piece of a video file on this computer, the file
// then deleted outside the app (a copy of the fixture, in the run's own
// folder). The piece says it's a hole, the edit isn't left waiting for
// footage, and it still renders, the hole as black and silence as long as
// the piece. Then the VOD removed with its video: keeping what the edit uses
// fails (its file is gone), and removed anyway, the piece stays a hole.

import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "holes",
  async run(app, { out, fixtures, log }) {
    await app.toLibrary();
    const dir = path.join(out, "holes");
    mkdirSync(dir, { recursive: true });
    const file = path.join(dir, "to-delete.mp3");
    copyFileSync(fixtures.landed, file);
    const { vods } = await app.invoke("add_files", { paths: [file] });
    const vod = vods[0];
    if (!vod) throw new Error("the file wasn't added");
    await until(
      "its length known",
      async () => (await app.invoke("list_vods")).find((v) => v.id === vod.id)?.duration > 3,
      MINUTE,
    );

    const edit = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", { editId: edit.id, vodId: vod.id, start: 1, end: 3, title: "Gone soon" });
    const piece = async () => (await app.invoke("get_edit", { id: edit.id })).pieces[0];
    if ((await piece()).hole) throw new Error("a piece of a file that's there is a hole");

    // Deleted outside the app. On Windows a file another process reads can't
    // be deleted, and the app may still be reading this one for a moment
    // (its waveform, its pictures): tried for a while, as a person would.
    rmSync(file, { maxRetries: 50, retryDelay: 200 });
    if (!(await piece()).hole) throw new Error("the piece of the deleted file isn't a hole");
    log("the file deleted: its piece is a hole");

    await app.invoke("render_edit_now", { id: edit.id });
    const done = await until(
      "the edit rendered",
      async () => {
        const e = (await app.invoke("list_edits")).find((e) => e.id === edit.id);
        if (e?.status === "error") throw new Error(`the render failed: ${e.error}`);
        return e?.status === "ready" && e;
      },
      MINUTE,
    );
    if (!existsSync(done.file_path)) throw new Error(`the render says ${done.file_path}, which isn't there`);
    log(`rendered with its hole: ${done.file_path}`);

    // Removed with its video: what the edit uses can't be kept (the file's
    // gone), so nothing is removed and the answer says why; anyway, the VOD
    // goes and the piece stays a hole.
    const failed = await app.invoke("delete_vod", { id: vod.id, deleteVideo: true, anyway: false });
    if (typeof failed !== "string") throw new Error(`removing it with its video answered ${JSON.stringify(failed)}`);
    if (!(await app.invoke("list_vods")).some((v) => v.id === vod.id)) throw new Error("the failed removal removed it");
    await app.invoke("delete_vod", { id: vod.id, deleteVideo: true, anyway: true });
    if ((await app.invoke("list_vods")).some((v) => v.id === vod.id))
      throw new Error("removed anyway, it's still listed");
    if (!(await piece()).hole) throw new Error("the piece of the VOD deleted anyway isn't a hole");
    log(`removing it with its video: "${failed}"; anyway, the piece a hole`);
  },
};
