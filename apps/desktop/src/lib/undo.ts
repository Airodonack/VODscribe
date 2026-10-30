// Undo and redo, from the keys (App.svelte) and from a toast's Undo after a
// removal (bulk.ts, EditView.svelte): one way, so each says what it did,
// offers the way back (Redo after an undo, Undo after a redo), says when
// there's nothing to undo, and does nothing on the website's demo.

import { api, type Doc } from "./api";
import { DEMO } from "./demo";
import { toast } from "./toasts.svelte";

/** Undoes (or redoes) the newest change in `doc`'s history, and says what it was. */
export function undoRedo(redo: boolean, doc: Doc | null) {
  if (!doc || DEMO) return;
  (redo ? api.redo(doc) : api.undo(doc)).then(
    (done) =>
      toast.show(
        done ? `${redo ? "Redone" : "Undone"}: ${done.label}` : redo ? "Nothing to redo" : "Nothing to undo",
        // The toast's button: on what was undone, whatever's open by then.
        done ? { label: redo ? "Undo" : "Redo", run: () => undoRedo(!redo, doc) } : undefined,
      ),
    (e) => toast.show(`Couldn't ${redo ? "redo" : "undo"}: ${e}`),
  );
}

/** Undoes the newest change in `doc`'s history (a toast's Undo). */
export const undoIn = (doc: Doc) => undoRedo(false, doc);
