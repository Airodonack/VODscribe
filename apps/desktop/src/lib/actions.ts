// What can be done with a VOD or an edit, wherever it shows: the library's
// rows and their right-click menus (Library.svelte, VodRow.svelte,
// EditRow.svelte), the player's header (Player.svelte, EditView.svelte).
// Each surface shows those it has room for, from this one list, so one
// doesn't have an action its sibling lacks. And removing, from a row, a
// header or several picked (bulk.ts), asks one way (`confirmRemove`) and
// goes one way (`removeEntries`).

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type Edit, type Vod } from "./api";
import { ask, choose } from "./ask";
import { edits } from "./edits.svelte";
import type { MenuItem } from "./Menu.svelte";
import { chaptersText, linksHaveTime, listed, siteName, watchUrl } from "./platform";
import { DELETE, REMOVE, removeVod } from "./removal.svelte";
import { count } from "./selection.svelte";
import { toast } from "./toasts.svelte";

/** What the actions are of. A VOD's link is to `at` seconds in (its start if not given). */
export type Target = { kind: "vod"; vod: Vod; at?: number } | { kind: "edit"; edit: Edit };

/** One action: a menu's item, with the name a surface finds it by. */
export type Action = MenuItem & { id: "folder" | "open" | "copy" | "add" | "remove" };

/** What a surface lends the actions it can't do on its own. */
export type Hooks = {
  /** Asks which edit to put it in (AddToEdit.svelte, anchored where the surface is). */
  addToEdit?: () => void;
  /** Told before a VOD goes: it may be the one open. */
  onremove?: (vodId: number) => void;
};

/** Whether `vod` plays from a file here, so its folder can be shown: a
 *  file's own, or the video downloaded for a site's VOD (`media`). */
export const hasFile = (vod: Vod) => vod.source_path !== null || vod.media;

/** Shows VOD `vod`'s file in its folder; what goes wrong, as a toast. */
export const showVodFile = (vod: Vod) => api.showFile(vod.id).catch((e) => toast.error(String(e)));

/** Shows edit `edit`'s rendered video in its folder. */
export const showEditFile = (edit: Edit) => api.showEdit(edit.id).catch((e) => (edits.error = String(e)));

/** Copies a link to VOD `vod` on its site, at `at` seconds where its links can say one. */
export async function copyVodLink(vod: Vod, at = 0): Promise<boolean> {
  return writeText(watchUrl(vod, at)).then(
    () => true,
    (e) => (console.error("copy failed", e), false),
  );
}

/** Copies `chapters` as a YouTube video's description lists them
 *  (platform.ts `chaptersText`): from a chapter's title box (MarkEditor.svelte)
 *  or the chapters pane's header (AnnotationsPane.svelte). What to say of it. */
export async function copyChapters(chapters: { start: number; text: string | null }[]): Promise<string> {
  try {
    await writeText(chaptersText(chapters));
    return chapters[0].start >= 1
      ? "Copied. YouTube shows chapters only when the first is at 0:00."
      : chapters.length < 3
        ? "Copied. YouTube shows chapters only when there are three or more."
        : "Copied, for the video's description on YouTube.";
  } catch (e) {
    return `Couldn't copy them: ${e}`;
  }
}

/** The actions of `target`, in the order a menu lists them. */
export function actionsFor(target: Target, hooks: Hooks = {}): Action[] {
  const out: Action[] = [];
  if (target.kind === "vod") {
    const { vod, at = 0 } = target;
    if (hasFile(vod)) out.push({ id: "folder", label: "Show in folder", icon: "folder", run: () => showVodFile(vod) });
    // A site's VOD, its video downloaded or not, links to its site.
    if (vod.source_path === null) {
      const site = siteName(vod);
      out.push({ id: "open", label: `Open on ${site}`, icon: "external", run: () => openUrl(watchUrl(vod, at)) });
      out.push({
        id: "copy",
        label: linksHaveTime(vod.platform) && at > 0 ? "Copy link to this moment" : "Copy link",
        icon: "copy",
        run: () => copyVodLink(vod, at).then((done) => toast.show(done ? "Link copied" : "Couldn't copy the link")),
      });
    }
    if (hooks.addToEdit) out.push({ id: "add", label: "Add to edit…", icon: "plus", run: hooks.addToEdit });
    out.push({
      id: "remove",
      label: "Remove…",
      icon: "trash",
      danger: true,
      run: () => void removeEntries([vod], [], { onremove: hooks.onremove }),
    });
  } else {
    const { edit } = target;
    if (edit.status === "ready" && edit.file_path)
      out.push({ id: "folder", label: "Show in folder", icon: "folder", run: () => showEditFile(edit) });
    if (hooks.addToEdit) out.push({ id: "add", label: "Add to edit…", icon: "plus", run: hooks.addToEdit });
    out.push({
      id: "remove",
      label: "Delete edit…",
      icon: "trash",
      danger: true,
      run: () => void removeEntries([], [edit]),
    });
  }
  return out;
}

/**
 * Asks whether to remove `vods` and delete `gone` (edits), naming one by its
 * title and several by how many: a VOD's transcript is erased and its clips
 * leave; an edit is deleted with its video. A VOD that edits staying use can
 * keep its video for them or have it deleted, all but the stretches they
 * use. What was answered, or null if nothing's to go.
 */
export async function confirmRemove(vods: Vod[], gone: Edit[]): Promise<{ deleteVideo: boolean } | null> {
  if (!vods.length && !gone.length) return null;
  const one = vods.length + gone.length === 1;
  const videos = gone.some((e) => e.file_path);
  if (!vods.length) {
    const sure = await ask(
      one
        ? `Delete the edit “${gone[0].title}”${videos ? " and its video" : ""}?`
        : `Delete ${count([[gone.length, "edit", "edits"]])}${videos ? " and their videos" : ""}?`,
      "Delete",
      "Keep",
    );
    return sure ? { deleteVideo: false } : null;
  }
  const what = one
    ? `“${vods[0].title}”`
    : count([
        [vods.length, "VOD", "VODs"],
        [gone.length, "edit", "edits"],
      ]);
  const single = vods.length === 1;
  const their = single ? "its" : "their";
  const vodText = ` ${one ? "Its transcript" : single ? "The VOD's transcript" : "The VODs' transcripts"} and any downloaded audio or video are erased from this computer, and ${their} clips leave VODscribe (their files stay in your Videos folder).`;
  const editText = gone.length
    ? ` ${gone.length === 1 ? "The edit is" : "The edits are"} deleted${videos ? `, with ${gone.length === 1 ? "its video" : "their videos"}` : ""}.`
    : "";
  // Edits staying that use a VOD going: its video can stay for them, or go.
  const using = [
    ...new Set(vods.flatMap((v) => edits.forVod(v.id).filter((e) => !gone.some((g) => g.id === e.id)))),
  ].map((e) => `“${e.title}”`);
  if (!using.length)
    return (await ask(`Remove ${what}?${vodText}${editText}`, "Remove", "Keep")) ? { deleteVideo: false } : null;
  const answer = await choose(
    `Remove ${what}?${vodText}${editText} ${single ? "Its" : "Their"} pieces in ${using.length === 1 ? "the edit" : "the edits"} ${listed(using)} stay: removed from the library, ${single ? "its video stays" : "their videos stay"} on this computer for them; deleted, only the stretches they use are kept.`,
    REMOVE,
    DELETE,
    "Keep",
  );
  return answer ? { deleteVideo: answer === DELETE } : null;
}

/**
 * Removes `vods` and deletes `gone` (edits) once `confirmRemove` says so:
 * one row's Remove, or several picked (bulk.ts). `onremove` is told of each
 * VOD before it goes (it may be the one open), `confirmed` once it's
 * answered. True if they went.
 */
export async function removeEntries(
  vods: Vod[],
  gone: Edit[],
  { onremove, confirmed }: { onremove?: (vodId: number) => void; confirmed?: () => void } = {},
): Promise<boolean> {
  const answer = await confirmRemove(vods, gone);
  if (!answer) return false;
  confirmed?.();
  for (const edit of gone) await edits.remove(edit.id);
  for (const vod of vods) {
    onremove?.(vod.id);
    await removeVod(vod, answer.deleteVideo, (message) => toast.error(`Couldn't remove “${vod.title}”: ${message}`));
  }
  return true;
}
