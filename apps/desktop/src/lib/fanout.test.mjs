// The event fan-out: node --test apps/desktop/src/lib/fanout.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";

const { fanned } = await import("./fanout.ts");

// A `listen` as Tauri's behaves: each registration completes asynchronously,
// and not in the order it was asked for (here, the reverse), so plain
// listeners are told in the order their registrations completed.
function tauriLike() {
  /** @type {{ name: string; handler: (e: { payload: unknown }) => void }[]} */
  const handlers = [];
  /** @type {(() => void)[]} */
  const pending = [];
  /** @type {(name: string, handler: (e: { payload: unknown }) => void) => Promise<() => void>} */
  const listen = (name, handler) =>
    new Promise((resolve) => {
      pending.push(() => {
        handlers.push({ name, handler });
        resolve(() => {
          const i = handlers.findIndex((h) => h.handler === handler);
          if (i >= 0) handlers.splice(i, 1);
        });
      });
    });
  const settle = () => {
    for (const finish of pending.splice(0).reverse()) finish();
  };
  /** @type {(name: string, payload: unknown) => void} */
  const emit = (name, payload) => {
    for (const h of [...handlers]) if (h.name === name) h.handler({ payload });
  };
  return { listen, settle, emit };
}

test("plain listeners are told in registration-completion order, not subscription order", async () => {
  const { listen, settle, emit } = tauriLike();
  /** @type {string[]} */
  const told = [];
  const a = listen("vod", () => told.push("cache"));
  const b = listen("vod", () => told.push("page"));
  settle();
  await Promise.all([a, b]);
  emit("vod", 1);
  assert.deepEqual(told, ["page", "cache"]);
});

test("fanned: subscribers are told in subscription order, whatever the registration order", async () => {
  const { listen, settle, emit } = tauriLike();
  const on = fanned(listen);
  /** @type {string[]} */
  const told = [];
  const a = on("vod", () => told.push("cache"));
  const b = on("vod", () => told.push("page"));
  const c = on("other", () => told.push("other"));
  settle();
  await Promise.all([a, b, c]);
  emit("vod", 1);
  assert.deepEqual(told, ["cache", "page"]);
});

test("fanned: one listener a name, an unlisten drops only its subscriber", async () => {
  const { listen, settle, emit } = tauriLike();
  let registrations = 0;
  /** @type {typeof listen} */
  const counting = (name, handler) => (registrations++, listen(name, handler));
  const on = fanned(counting);
  /** @type {[string, unknown][]} */
  const told = [];
  const a = on("vod", (e) => told.push(["cache", e.payload]));
  const b = on("vod", (e) => told.push(["page", e.payload]));
  settle();
  const [unA, unB] = await Promise.all([a, b]);
  assert.equal(registrations, 1);
  unA();
  emit("vod", 2);
  assert.deepEqual(told, [["page", 2]]);
  unB();
  emit("vod", 3);
  assert.deepEqual(told, [["page", 2]]);
});
