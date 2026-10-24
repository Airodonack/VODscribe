// Stands in for @tauri-apps/api/event. Nothing happens in the background of
// the demo; on a VOD's page, the stand-ins say when its chat arrives (annotations.ts).
export type UnlistenFn = () => void;

type Handler = (e: { payload: unknown }) => void;
const handlers = new Map<string, Set<Handler>>();

export async function listen<T>(event: string, handler: (e: { payload: T }) => void): Promise<UnlistenFn> {
  const set = handlers.get(event) ?? new Set();
  handlers.set(event, set);
  set.add(handler as Handler);
  return () => set.delete(handler as Handler);
}

/** Tells the page's listeners of `event`, as the app's backend would. */
export function emit(event: string, payload: unknown) {
  for (const handler of handlers.get(event) ?? []) handler({ payload });
}
