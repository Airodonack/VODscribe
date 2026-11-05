// The changes sent to the app and not answered yet. The app may take two
// commands in another order than they were sent (each runs as a task of its
// own), so an undo right after a change (a mark nudged, then Ctrl+Z at once)
// could undo the change before it; undo.ts waits for these first.

const inFlight = new Set<Promise<unknown>>();

/** Notes `change` as being saved until it settles; returns it. */
export function saving<T>(change: Promise<T>): Promise<T> {
  const settled = change.then(
    () => {},
    () => {},
  );
  inFlight.add(settled);
  settled.then(() => inFlight.delete(settled));
  return change;
}

/** Resolves once every change sent so far has been answered, or failed. */
export const saved = (): Promise<unknown> => Promise.all(inFlight);
