// What a failure leaves to look at, as text: the page's errors, the media
// element's state, the app's own player's (through its e2e command), the
// time shown, whether the page has the keyboard, the app's process, the
// page's console errors and its text. A stall then carries its cause: a
// media element that never buffered, a native player whose decoder ended or
// whose thread is stuck, a window without focus, an app already gone.
//
// Used by App's helpers that fail (a click on what never came, a page that
// can't be given the keyboard), and meant for run.mjs's diagnosis of a
// failure too; formatDump is pure, for its tests.

/** How long the app's player has to answer: a stuck one mustn't hang the dump. */
const PLAYER_WAIT_MS = 2000;
/** How long the page has to answer at all. */
const PAGE_WAIT_MS = 10_000;

/** Run in the page: what the dump says of it, as one object. */
const PAGE_STATE = `
  const media = document.querySelector("video, audio");
  const ranges = (r) => Array.from({ length: r.length }, (_, i) => [r.start(i), r.end(i)]);
  const finite = (n) => (Number.isFinite(n) ? n : String(n));
  const internals = window.__TAURI_INTERNALS__;
  const native = internals
    ? await Promise.race([
        internals.invoke("e2e_player_state").then(
          (state) => ({ state }),
          (e) => ({ error: String((e && e.message) || e) }),
        ),
        new Promise((r) => setTimeout(() => r({ error: "no answer in " + arguments[0] + " ms" }), arguments[0])),
      ])
    : { error: "no __TAURI_INTERNALS__ in the page" };
  return {
    errors: [...document.querySelectorAll(".error-text")].map((e) => e.textContent.trim()).filter(Boolean),
    media: media && {
      tag: media.tagName.toLowerCase(),
      src: media.currentSrc,
      readyState: media.readyState,
      networkState: media.networkState,
      paused: media.paused,
      seeking: media.seeking,
      ended: media.ended,
      currentTime: media.currentTime,
      duration: finite(media.duration),
      buffered: ranges(media.buffered),
      error: media.error && media.error.code + " " + media.error.message,
    },
    native,
    timeShown: document.querySelector("button.time")?.textContent.trim() ?? null,
    visibility: document.visibilityState,
    focused: document.hasFocus(),
    console: window.__e2e?.console ?? [],
    text: document.body.innerText,
  };`;

const seconds = (s) => (typeof s === "number" ? `${s.toFixed(3)} s` : String(s));

/** The media element's line. */
function mediaLine(media) {
  if (!media) return "media element: no <video> or <audio>";
  const buffered = media.buffered.length
    ? media.buffered.map(([a, b]) => `${a.toFixed(3)}–${b.toFixed(3)}`).join(", ")
    : "nothing";
  return (
    `media element: <${media.tag}> ${media.paused ? "paused" : "playing"}${media.seeking ? ", seeking" : ""}` +
    `${media.ended ? ", ended" : ""} at ${seconds(media.currentTime)} of ${seconds(media.duration)}; ` +
    `readyState ${media.readyState}, networkState ${media.networkState}; buffered ${buffered}; ` +
    `error ${media.error ?? "none"}; src ${media.src || "none"}`
  );
}

/** The app's own player's line, from `e2e_player_state` (player.rs `Seen`). */
function nativeLine(native) {
  if (!native) return "native player: not asked";
  if (native.error) {
    const missing = /e2e_player_state.*not (found|allowed)|not found.*e2e_player_state/i.test(native.error);
    return `native player: ${missing ? "not in this build (no e2e feature)" : `couldn't be asked: ${native.error}`}`;
  }
  const { open, doc, player } = native.state;
  if (open === null) return "native player: its lock is held (a stuck thread, or an open under way)";
  if (!open) return "native player: none open";
  const what = doc ? `${doc.kind} ${doc.id}` : "?";
  const s = player.state;
  const clock = s
    ? `${s.playing ? "playing" : "paused"}${s.ended ? ", ended" : ""} at ${seconds(s.time)} of ` +
      `${seconds(player.duration)}, epoch ${s.epoch}, ${s.ready ? "ready" : "waiting for its first frame"}, ` +
      `speed ${s.speed}, ${s.sound_output ? "a sound output" : "no sound output"}`
    : "its clock's lock is held (a stuck thread)";
  const frame = !player.frame_known
    ? "the frame's lock is held"
    : player.frame_pts === null
      ? "no frame"
      : `last frame pts ${seconds(player.frame_pts)} (shown at ${seconds(player.shown)}, ` +
        `${player.last_frame_ms_ago} ms ago)`;
  return (
    `native player: ${what} open, ${clock}; ${frame}; decoder ${player.decoding ? "running" : "none"}; ` +
    `thread ${player.thread_alive ? "alive" : "ended"}; ${player.shown_count} frames shown, ${player.dropped} dropped`
  );
}

/** A clock time to the millisecond (UTC), as the app's own e2e log has it. */
const timeOfDay = (ms) => new Date(ms).toISOString().slice(11, 23);

/**
 * The app's process line, from a driver's `proc` ({ pid, closedAt,
 * exitedAt, exit }, lib/drivers.mjs): whether it runs, and when it ended
 * against when it was asked to.
 */
export function appLine(proc, now = Date.now()) {
  if (!proc?.pid) return "app: its process isn't known here";
  if (proc.exitedAt === null || proc.exitedAt === undefined)
    return `app: pid ${proc.pid}, running${proc.closedAt ? `, ${now - proc.closedAt} ms after it was asked to quit` : ""}`;
  const how = proc.exit === null || proc.exit === undefined ? "" : ` (${proc.exit})`;
  const asked = proc.closedAt
    ? `${proc.exitedAt - proc.closedAt} ms after it was asked to quit`
    : "without being asked to quit";
  return `app: pid ${proc.pid} exited${how} at ${timeOfDay(proc.exitedAt)} UTC, ${asked}`;
}

/**
 * The dump's text from what the page said (PAGE_STATE's object, or an
 * Error if it couldn't be asked) and the app's process line; with
 * `text: false`, without the page's text (for a log line).
 */
export function formatDump(page, app, { text = true } = {}) {
  if (!page || page instanceof Error)
    return [`(couldn't read the page: ${page?.message ?? "no answer"})`, app].join("\n");
  return [
    ...(page.errors.length ? [`the page shows: ${page.errors.join(" | ")}`] : []),
    mediaLine(page.media),
    nativeLine(page.native),
    `time shown: ${page.timeShown ?? "none"}`,
    `page: ${page.visibility}${page.focused ? ", focused" : ", not focused"}`,
    app,
    `console errors: ${JSON.stringify(page.console)}`,
    ...(text ? ["", page.text] : []),
  ].join("\n");
}

/** The dump of the app `driver` drives; never throws, and never waits more than about ten seconds. */
export async function pageDump(driver, options) {
  let timer;
  const page = await Promise.race([
    driver.run(PAGE_STATE, [PLAYER_WAIT_MS]),
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`no answer in ${PAGE_WAIT_MS / 1000} s`)), PAGE_WAIT_MS);
    }),
  ])
    .catch((e) => e)
    .finally(() => clearTimeout(timer));
  return formatDump(page, appLine(driver.proc), options);
}
