// The file host the app asks for a VOD's published annotations, and the
// image server its page asks for the cast's pictures, played by the run on
// this computer (crates/core annotations.rs: VODSCRIBE_ANNOTATIONS_URL and
// VODSCRIBE_AVATAR_URL name it), so no scenario reaches the real ones. The run starts it before
// the first app, on a port of its own; it answers nobody (the connection
// dropped, as when offline) until a scenario serves something with it, and
// again after.

import { createHash } from "node:crypto";
import { createServer } from "node:http";

/** The variable the app reads the file host's address from. */
export const FILE_HOST_ENV = "VODSCRIBE_ANNOTATIONS_URL";
/** The variable the app reads the image server's address from: the same host, under /avatar/. */
export const AVATAR_HOST_ENV = "VODSCRIBE_AVATAR_URL";

/** What answers now, and what it has been asked since it began to. */
let serving = null;

/**
 * Starts the file host, and points the apps started from here on at it
 * (through this process's environment, which the drivers pass on).
 */
export async function startFileHost() {
  const server = createServer(async (req, res) => {
    if (!serving) return req.socket.destroy();
    const since = req.headers["if-none-match"];
    serving.requests.push({ method: req.method, path: req.url, ...(since && { since }) });
    const { status, headers = {}, body = "" } = await serving.answer(req.method, req.url, since);
    res.writeHead(status, { "Content-Length": Buffer.byteLength(body), ...headers });
    res.end(req.method === "HEAD" ? undefined : body);
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  // The run ends with process.exit; nothing waits on this.
  server.unref();
  process.env[FILE_HOST_ENV] = process.env[AVATAR_HOST_ENV] = `http://127.0.0.1:${server.address().port}`;
}

/**
 * Has the file host answer each request with `answer(method, path, since)`
 * (`since`: its If-None-Match, if any):
 * `{ status, headers?, body? }` (the body a string or a Buffer), or a
 * promise of one, to hold an answer back. Returns what it's asked from now on
 * (`requests`, each `{ method, path }`, and `since`, its If-None-Match, if
 * it has one) and `stop()`, after which it
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

/** Where the app asks for the filter of published annotations, at every start
 *  and on the library's "Check all VODs for annotations" (crates/core
 *  annotations_filter.rs). Unserved, as by most scenarios, it's 404 and the
 *  app asks nothing more. */
export const FILTER_PATH = "/annotations/filter.bin";

/** Whether a request is for something other than the filter: what a scenario
 *  about one VOD's file counts. */
export const notFilter = (request) => request.path !== FILTER_PATH;

/** A file's version as the filter names it: its bytes' MD5, which the file
 *  host gives as its ETag (quoted). */
export const md5 = (body) => createHash("md5").update(body).digest("hex");

/**
 * The filter file holding `keys` (annotations_filter.rs has the format):
 * sized for a 1% false-positive rate, as the real one is made.
 */
export function annotationsFilter(keys) {
  const bits = Math.ceil(Math.max(64, Math.ceil((keys.length * 9586) / 1000)) / 8) * 8;
  const hashes = 7;
  const data = Buffer.alloc(bits / 8);
  for (const key of keys) for (const bit of positions(key, bits, hashes)) data[bit >> 3] |= 1 << (bit & 7);
  const header = Buffer.alloc(28);
  header.write("VSAF", 0, "latin1");
  header.writeUInt32LE(1, 4);
  header.writeBigUInt64LE(BigInt(bits), 8);
  header.writeUInt32LE(hashes, 16);
  header.writeBigUInt64LE(BigInt(keys.length), 20);
  return Buffer.concat([header, data]);
}

/** Key `key`'s bits in a filter of `bits` bits and `hashes` hashes. */
export function positions(key, bits, hashes) {
  const d = createHash("sha256").update(key, "utf8").digest();
  const h1 = d.readBigUInt64LE(0);
  const h2 = d.readBigUInt64LE(8) | 1n;
  const mask = (1n << 64n) - 1n;
  return Array.from({ length: hashes }, (_, i) => Number(((h1 + BigInt(i) * h2) & mask) % BigInt(bits)));
}
