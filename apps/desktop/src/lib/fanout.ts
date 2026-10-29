// One listener per event name, fanning out to its subscribers in the order
// they subscribed. Tauri's `listen` registers each subscriber asynchronously,
// so two subscribers to one event aren't told in the order they subscribed:
// on CI's release build the page re-read a VOD's marks on "vod" (an
// annotations import changes the VOD) from the kept cache (kept.ts) before
// the cache's own subscriber let them go, and the chapters never showed
// (2026-10-29). Through here, api.ts's cache subscribes first, at load, and
// is told first.

export type Handler<E> = (e: E) => void;
export type Unlisten = () => void;
export type Listen<E> = (name: string, handler: Handler<E>) => Promise<Unlisten>;

/** `listen`, with every subscriber to a name told in the order they subscribed. */
export function fanned<E>(listen: Listen<E>): Listen<E> {
  const fans = new Map<string, { handlers: Set<Handler<E>>; ready: Promise<Unlisten> }>();
  return (name, handler) => {
    let fan = fans.get(name);
    if (!fan) {
      const handlers = new Set<Handler<E>>();
      fan = {
        handlers,
        ready: listen(name, (e) => {
          for (const h of [...handlers]) h(e);
        }),
      };
      fans.set(name, fan);
    }
    const { handlers, ready } = fan;
    handlers.add(handler);
    return ready.then(() => () => {
      handlers.delete(handler);
    });
  };
}
