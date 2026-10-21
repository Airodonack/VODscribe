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
