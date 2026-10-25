// The file host the app asks for a VOD's published annotations, played by
// the run on this computer (crates/core annotations.rs: VODSCRIBE_ANNOTATIONS_URL
// names it), so no scenario reaches the real one. The run starts it before
// the first app, on a port of its own; it answers nobody (the connection
// dropped, as when offline) until a scenario serves something with it, and
// again after.

import { createServer } from "node:http";

/** The variable the app reads the file host's address from. */
export const FILE_HOST_ENV = "VODSCRIBE_ANNOTATIONS_URL";

/** What answers now, and what it has been asked since it began to. */
let serving = null;

/**
 * Starts the file host, and points the apps started from here on at it
 * (through this process's environment, which the drivers pass on).
 */
export async function startFileHost() {
  const server = createServer((req, res) => {
    if (!serving) return req.socket.destroy();
    serving.requests.push({ method: req.method, path: req.url });
    const { status, headers = {}, body = "" } = serving.answer(req.method, req.url);
    res.writeHead(status, { "Content-Length": Buffer.byteLength(body), ...headers });
    res.end(req.method === "HEAD" ? undefined : body);
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  // The run ends with process.exit; nothing waits on this.
  server.unref();
  process.env[FILE_HOST_ENV] = `http://127.0.0.1:${server.address().port}`;
}

/**
 * Has the file host answer each request with `answer(method, path)`:
 * `{ status, headers?, body? }`. Returns what it's asked from now on
 * (`requests`, each `{ method, path }`) and `stop()`, after which it
 * answers nobody again.
 */
export function serveFiles(answer) {
  const mine = { answer, requests: [] };
  serving = mine;
  return {
    requests: mine.requests,
    stop: () => {
      if (serving === mine) serving = null;
    },
  };
}
