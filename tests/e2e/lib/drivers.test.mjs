import assert from "node:assert/strict";
import { test } from "node:test";

import { sessionGone } from "./drivers.mjs";

test("a session WebKitWebDriver no longer has is gone", () => {
  // As CI's run got it, every poll after the app's web process died.
  assert.match(sessionGone({ error: "invalid session id", message: "", stacktrace: "" }), /session is gone/);
  assert.match(
    sessionGone({ error: "invalid session id", message: "session deleted because of page crash or hang." }),
    /session is gone.*: session deleted because of page crash or hang\.$/,
  );
});

test("tauri-driver not listening is a session gone", () => {
  const refused = new TypeError("fetch failed", {
    cause: Object.assign(new Error("connect"), { code: "ECONNREFUSED" }),
  });
  assert.match(sessionGone(refused), /tauri-driver/);
});

test("a command that failed in a live session is only that command", () => {
  // A page promise rejected with a string, the first answer once the web
  // process had died, and a script's own error: the session may live on.
  assert.equal(sessionGone({ error: "unknown error", message: "Could not parse script result" }), null);
  assert.equal(sessionGone({ error: "javascript error", message: "ReferenceError: x is not defined" }), null);
  assert.equal(sessionGone({ error: "no such element", message: "" }), null);
  // fetch's own limit, or a connection dropped mid-answer: not known to be gone.
  const timedOut = new TypeError("fetch failed", {
    cause: Object.assign(new Error("headers"), { code: "UND_ERR_HEADERS_TIMEOUT" }),
  });
  assert.equal(sessionGone(timedOut), null);
  assert.equal(sessionGone(undefined), null);
});
