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
    getComputedStyle: () => ({ backgroundColor: "rgba(0, 0, 0, 0)", backgroundImage: "none", opacity: "1" }),
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
