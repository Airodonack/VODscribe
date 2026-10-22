// The embed shim (embed_shim.js) in a stand-in of YouTube's embed frame,
// for what a real ad can't be had for: node --test apps/desktop/src-tauri/src/embed_shim.test.mjs
// The frame's page is faked as far as the shim reaches: its MediaSource, a
// <video>, the player's root (whose classes say an ad shows), postMessage.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const SHIM = readFileSync(new URL("./embed_shim.js", import.meta.url), "utf8");

/** A frame at YouTube's embed, the shim run in it; what it posted, and the means to drive it. */
function frame({ name = "" } = {}) {
  const posted = [];
  const listeners = [];
  // The browser's own MediaSource and media element, as far as the shim uses them.
  class RealMediaSource extends EventTarget {
    constructor() {
      super();
      this.readyState = "closed";
      this.duration = NaN;
      this.buffers = [];
    }
    get sourceBuffers() {
      return this.buffers;
    }
    addSourceBuffer(type) {
      const b = { type, real: true };
      this.buffers.push(b);
      return b;
    }
    endOfStream() {
      this.readyState = "ended";
    }
  }
  const realUrls = new Map();
  class HTMLMediaElement extends EventTarget {
    constructor() {
      super();
      this.attached = null;
      this.tagName = "VIDEO";
      this.className = "";
      this.parentElement = null;
      const props = new Map();
      this.style = {
        getPropertyValue: (n) => props.get(n)?.[0] ?? "",
        getPropertyPriority: (n) => props.get(n)?.[1] ?? "",
        setProperty: (n, v, p = "") => (v ? props.set(n, [v, p]) : props.delete(n)),
      };
    }
    getBoundingClientRect() {
      return { left: 0, top: 0, width: 0, height: 0 };
    }
    get currentTime() {
      return "real";
    }
    play() {
      return Promise.resolve();
    }
    pause() {}
  }
  Object.defineProperty(HTMLMediaElement.prototype, "src", {
    configurable: true,
    get() {
      return this.attached;
    },
    set(v) {
      this.attached = v;
      const source = realUrls.get(v);
      if (source) queueMicrotask(() => ((source.readyState = "open"), source.dispatchEvent(new Event("sourceopen"))));
    },
  });
  const root = { classList: new Set(["html5-video-player"]) };
  const document = {
    querySelector: (selector) =>
      selector.includes("ad-showing") && (root.classList.has("ad-showing") || root.classList.has("ad-interrupting"))
        ? root
        : null,
    documentElement: {},
    elementsFromPoint: () => [],
    addEventListener: () => {},
  };
  const parent = {
    postMessage: (data) => posted.push(data.vodscribeShim),
  };
  const window = {
    name,
    parent,
    MediaSource: RealMediaSource,
    addEventListener: (type, fn) => listeners.push([type, fn]),
  };
  const context = vm.createContext({
    window,
    document,
    location: {
      hostname: "www.youtube-nocookie.com",
      pathname: "/embed/x",
      hash: "",
      href: "https://x",
      origin: "https://www.youtube-nocookie.com",
    },
    URL: {
      createObjectURL: (object) => {
        const url = `blob:real-${realUrls.size + 1}`;
        realUrls.set(url, object);
        return url;
      },
    },
    HTMLMediaElement,
    Element: { prototype: { setAttribute() {} } },
    EventTarget,
    Event,
    DOMException,
    DataView,
    Uint8Array,
    Map,
    Set,
    Promise,
    performance,
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    getComputedStyle: () => ({
      backgroundColor: "rgba(0, 0, 0, 0)",
      backgroundImage: "none",
      opacity: "1",
      colorScheme: "normal",
    }),
    queueMicrotask,
  });
  vm.runInContext(SHIM, context);
  // The page telling the frame something (embed.ts's bridge).
  const tell = (data) => {
    for (const [type, fn] of listeners) if (type === "message") fn({ source: parent, data });
  };
  /** A source their player makes, attached to `video`, as it would. */
  const attach = (video) => {
    const source = new window.MediaSource();
    video.src = context.URL.createObjectURL(source);
    return source;
  };
  return { window, root, posted, tell, attach, video: new HTMLMediaElement() };
}

const settle = () => new Promise((r) => setTimeout(r, 5));

test("the VOD's own source is taken over: the app's player plays it", async () => {
  const f = frame();
  const source = f.attach(f.video);
  await settle();
  assert.match(f.video.src, /vodscribe-/);
  assert.equal(typeof f.video.currentTime, "number");
  assert.equal(source.readyState, "open");
  // Their element, which shows nothing now, is see-through (a GPU draws an empty one black).
  assert.equal(f.video.style.getPropertyValue("opacity"), "0");
  // Their play is the app's player's play.
  f.video.play();
  assert.ok(f.posted.some((m) => m.kind === "media" && m.event === "play"));
  f.video.pause();
  clearInterval(f.video.media.controls);
});

test("while an ad shows, a new source plays in their element, not ours", async () => {
  const f = frame();
  f.attach(f.video);
  await settle();
  f.video.play();
  // YouTube's ad: the same element, a new MediaSource, the player marked.
  f.root.classList.add("ad-showing");
  const ad = f.attach(f.video);
  await settle();
  assert.match(f.video.src, /^blob:real-/, "their element plays a real source");
  assert.equal(f.video.style.getPropertyValue("opacity"), "", "and shows again");
  assert.equal(f.video.currentTime, "real", "the element's own state again");
  assert.equal(ad.readyState, "open", "their player hears sourceopen through ours");
  const buffer = ad.addSourceBuffer('video/webm; codecs="vp9"');
  assert.equal(buffer.real, true, "its buffers are real: nothing of the ad goes to the app");
  assert.ok(f.posted.some((m) => m.kind === "ad" && m.phase === "handed"));
  assert.ok(
    f.posted.some((m) => m.kind === "media" && m.event === "pause"),
    "the app's player stops while the ad plays",
  );
  // The ad over, the VOD's next source is ours again.
  f.root.classList.delete("ad-showing");
  f.attach(f.video);
  await settle();
  assert.match(f.video.src, /vodscribe-/);
  assert.equal(typeof f.video.currentTime, "number");
  clearInterval(f.video.media.controls);
});

test("the page's word that an ad shows counts too (their marking may come late)", async () => {
  const f = frame();
  f.tell({ vodscribeAd: true });
  f.attach(f.video);
  await settle();
  assert.match(f.video.src, /^blob:real-/);
  f.tell({ vodscribeAd: false });
  f.attach(f.video);
  await settle();
  assert.match(f.video.src, /vodscribe-/);
  clearInterval(f.video.media.controls);
});

test("a source whose length isn't the VOD's is reported", async () => {
  const f = frame();
  f.tell({ vodscribeVod: { duration: 70 } });
  const main = f.attach(f.video);
  await settle();
  main.duration = 70.2;
  assert.ok(!f.posted.some((m) => m.kind === "mismatch"), "the VOD's own, near enough");
  main.duration = 15;
  assert.ok(f.posted.some((m) => m.kind === "mismatch" && m.duration === 15));
  clearInterval(f.video.media.controls);
});

test("reloaded plain, the shim does nothing", () => {
  const f = frame({ name: "vodscribe-plain" });
  assert.equal(f.window.MediaSource.name, "RealMediaSource");
  assert.deepEqual(
    f.posted.map((m) => m.kind),
    ["plain"],
  );
});

/** An MP4 box: size, type, then `parts` (byte arrays or 32-bit numbers). */
function box(type, ...parts) {
  const body = parts.flatMap((p) =>
    typeof p === "number" ? [p >>> 24, (p >>> 16) & 255, (p >>> 8) & 255, p & 255] : [...p],
  );
  const size = 8 + body.length;
  return [
    size >>> 24,
    (size >>> 16) & 255,
    (size >>> 8) & 255,
    size & 255,
    ...[...type].map((c) => c.charCodeAt(0)),
    ...body,
  ];
}
/** An init segment: a timescale of 1000, and frames 33 long by default. */
const INIT = [
  ...box("ftyp", [...Buffer.from("isom")]),
  ...box("moov", box("trak", box("mdia", box("mdhd", 0, 0, 0, 1000, 0))), box("mvex", box("trex", 0, 1, 1, 33, 0, 0))),
];
/** A media segment from `base` (in 1/1000 s) holding `count` frames, and its data. */
const segment = (base, count) => [
  ...box("moof", box("traf", box("tfdt", 0, base), box("trun", 0, count))),
  ...box("mdat", [1, 2, 3]),
];

test("an fMP4 append says the times it holds, and whether it ends whole", async () => {
  const f = frame();
  const source = f.attach(f.video);
  await settle();
  const buffer = source.addSourceBuffer('video/mp4; codecs="avc1"');
  const appended = () => f.posted.filter((m) => m.kind === "append");
  buffer.appendBuffer(new Uint8Array([...INIT, ...segment(5000, 30)]));
  await settle();
  const [first] = appended();
  assert.equal(first.init, true);
  assert.equal(first.complete, true);
  assert.equal(first.ranges.length, 1);
  assert.ok(Math.abs(first.ranges[0][0] - 5) < 1e-9 && Math.abs(first.ranges[0][1] - 5.99) < 1e-9);
  // A segment split across two appends: its times come with the second, the first not whole.
  const next = segment(5990, 30);
  buffer.appendBuffer(new Uint8Array(next.slice(0, 20)));
  await settle();
  buffer.appendBuffer(new Uint8Array(next.slice(20)));
  await settle();
  const [, half, rest] = appended();
  assert.equal(JSON.stringify([half.ranges, half.complete]), "[[],false]");
  assert.ok(Math.abs(rest.ranges[0][0] - 5.99) < 1e-9 && rest.complete);
  clearInterval(f.video.media.controls);
});

/** What the frame says an fMP4 `init` and `media` appended together hold. */
async function heldBy(init, media) {
  const f = frame();
  const source = f.attach(f.video);
  await settle();
  source.addSourceBuffer('video/mp4; codecs="avc1"').appendBuffer(new Uint8Array([...init, ...media]));
  await settle();
  clearInterval(f.video.media.controls);
  // Arrays of this realm, to compare (the frame's are another's), rounded off.
  return Array.from(f.posted.find((m) => m.kind === "append").ranges, ([a, b]) => [+a.toFixed(6), +b.toFixed(6)]);
}
/** Four frames 33 long from `base`, decoded I P B B and shown I B B P, each with its composition offset. */
const reordered = (base, offsets, version = 0) => [
  ...box("moof", box("traf", box("tfdt", 0, base), box("trun", (version << 24) | 0x800, offsets.length, ...offsets))),
  ...box("mdat", [1, 2, 3]),
];

test("an fMP4 append's times are when its frames show, not when they decode", async () => {
  // As Twitch's H.264: decoded from 0, its first frame shown 2 frames on (no edit list).
  // Shown at 0.066, 0.165, 0.099, 0.132: from 0.066 till the last, at 0.165, ends.
  assert.deepEqual(await heldBy(INIT, reordered(0, [66, 132, 33, 33])), [[0.066, 0.198]]);
  // Decode times alone would say it starts 2 frames early, which a browser's buffered never says.
  assert.notDeepEqual(await heldBy(INIT, reordered(0, [66, 132, 33, 33])), [[0, 0.132]]);
  // Later in the VOD, the same.
  assert.deepEqual(await heldBy(INIT, reordered(5000, [66, 132, 33, 33])), [[5.066, 5.198]]);
  // Version 1's offsets are signed: a writer shows the first frame at its decode time.
  assert.deepEqual(await heldBy(INIT, reordered(5000, [0, 66, -33, -33], 1)), [[5, 5.132]]);
});

/**
 * The test VOD's (2901646294) first 2 s of picture: Twitch's H.264 from its
 * first HLS segment, put in fMP4 by ffmpeg (-c copy -use_editlist 0) as their
 * player hands it over: its init and first moof, the frames themselves (mdat)
 * left out, as the shim never reads them. ffprobe: first decoded at 0, first
 * shown at 0.067; the last shown at 2.034, for 1/30 s.
 */
const TWITCH = Buffer.from(
  "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",
  "base64",
);

test("Twitch's own frames are said from when the first shows", async () => {
  assert.deepEqual(await heldBy([...TWITCH], []), [[0.067, 2.067]]);
});

test("an fMP4 init's edit list moves its times by where it starts the media", async () => {
  /** INIT with an edit list starting the media at `mediaTime` (in version 1's wide fields if `wide`). */
  const edited = (mediaTime, wide = false) => [
    ...box("ftyp", [...Buffer.from("isom")]),
    ...box(
      "moov",
      box(
        "trak",
        box(
          "edts",
          wide
            ? box("elst", 1 << 24, 1, 0, 4000, 0, mediaTime, 0x10000)
            : box("elst", 0, 2, 0, -1, 0x10000, 4000, mediaTime, 0x10000),
        ),
        box("mdia", box("mdhd", 0, 0, 0, 1000, 0)),
      ),
      box("mvex", box("trex", 0, 1, 1, 33, 0, 0)),
    ),
  ];
  // The reordering delay edited out, as most writers do: shown from 0. (An empty edit first is passed over.)
  assert.deepEqual(await heldBy(edited(66), reordered(0, [66, 132, 33, 33])), [[0, 0.132]]);
  // Its first second edited out: what decodes from 5 s shows from 4 s.
  assert.deepEqual(await heldBy(edited(1000, true), segment(5000, 30)), [[4, 4.99]]);
});

test("a box with no size doesn't stop the frame", async () => {
  const f = frame();
  const source = f.attach(f.video);
  await settle();
  const buffer = source.addSourceBuffer('video/mp4; codecs="avc1"');
  buffer.appendBuffer(new Uint8Array([...INIT, 0, 0, 0, 0, ...[..."moof"].map((c) => c.charCodeAt(0)), 9, 9]));
  await settle();
  assert.ok(
    f.posted.some((m) => m.kind === "append"),
    "the append went through",
  );
  clearInterval(f.video.media.controls);
});

test("played at its end, it starts again from the beginning", async () => {
  const f = frame();
  f.attach(f.video);
  await settle();
  f.video.media.ended = true;
  f.video.media.time = 70;
  f.video.play();
  assert.ok(f.posted.some((m) => m.kind === "media" && m.event === "seek" && m.time === 0));
  f.video.pause();
  clearInterval(f.video.media.controls);
});

test("asked to play, its time stands, waiting, till the app's player runs", async () => {
  const f = frame();
  const source = f.attach(f.video);
  await settle();
  const buffer = source.addSourceBuffer('video/mp4; codecs="avc1"');
  buffer.appendBuffer(new Uint8Array([...INIT, ...segment(0, 300)]));
  await settle();
  const events = [];
  for (const name of ["playing", "waiting"]) f.video.addEventListener(name, () => events.push(name));
  f.video.play();
  // Their player has the data, the app's player hasn't started (it took 6 s on a desktop).
  await new Promise((r) => setTimeout(r, 900));
  assert.ok(f.video.currentTime < 0.05, `stood at the start, not run on to ${f.video.currentTime}`);
  assert.ok(!events.includes("playing"), `not playing yet: ${events}`);
  // The app's player runs: so does the element.
  f.tell({ vodscribeClock: { time: 0, playing: true } });
  await settle();
  assert.equal(events.at(-1), "playing");
  await new Promise((r) => setTimeout(r, 200));
  assert.ok(f.video.currentTime > 0.1, `runs on with it: ${f.video.currentTime}`);
  f.video.pause();
  clearInterval(f.video.media.controls);
});
