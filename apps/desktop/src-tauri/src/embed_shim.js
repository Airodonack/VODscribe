// Runs in every frame of the app's window; acts only inside a site's
// embedded player (YouTube's, Twitch's), and not in one the page loaded to
// play by itself (named "vodscribe-plain": embed.ts).
// The media seam their player plays through is ours. MediaSource, its
// SourceBuffers, and the state of the <video> it's attached to are
// stand-ins: their player appends its segments to us, and we hand them to
// the page (and on to the app's player, embed.rs, which draws the picture
// under their frame and plays the sound); the element's time, buffered
// ranges, ready state and events come from a clock of ours, kept in step
// with the app's player's, as a browser's would. Their element shows
// nothing, and the backgrounds behind it are cleared, so the picture under
// the frame shows through. Their player is otherwise untouched: its
// controls, overlays and ads draw as they do. An ad played through a source
// of its own is handed to a real MediaSource, so their element plays it.
(() => {
  const host = location.hostname;
  const youtube = /(^|\.)youtube(-nocookie)?\.com$/.test(host) && location.pathname.startsWith("/embed/");
  const twitch = host === "player.twitch.tv";
  if (!youtube && !twitch) return;
  const site = youtube ? "youtube" : "twitch";
  const post = (message, transfer) => {
    try {
      window.parent.postMessage({ vodscribeShim: { site, ...message } }, "*", transfer || []);
    } catch (e) {
      // Nothing to tell it with.
    }
  };
  // Their plain player (the page's choice, or it fell back: embed.ts): nothing of ours.
  if (window.name === "vodscribe-plain" || location.hash.includes("vodscribe-plain")) {
    post({ kind: "plain" });
    return;
  }
  post({ kind: "loaded", url: location.href.slice(0, 120) });

  // --- What the page tells us -------------------------------------------
  /** The VOD's length (seconds), once the page says; 0 until then. */
  let vodDuration = 0;
  /** An ad shows (the page heard their onAdStart, not yet onAdEnd). */
  let adTold = false;
  /** An ad shows: as the page says, or as their player marks itself (YouTube). */
  const adShowing = () =>
    adTold || !!document.querySelector(".html5-video-player.ad-showing, .html5-video-player.ad-interrupting");

  // --- Time ranges ------------------------------------------------------
  class Ranges {
    constructor(list) {
      this.list = list;
    }
    get length() {
      return this.list.length;
    }
    start(i) {
      if (i >= this.list.length) throw new DOMException("index", "IndexSizeError");
      return this.list[i][0];
    }
    end(i) {
      if (i >= this.list.length) throw new DOMException("index", "IndexSizeError");
      return this.list[i][1];
    }
  }
  /** `list` with [a, b] added, merged where they touch (within a frame or so). */
  const addRange = (list, a, b) => {
    const all = [...list, [a, b]].sort((x, y) => x[0] - y[0]);
    const out = [];
    for (const [s, e] of all) {
      const last = out[out.length - 1];
      if (last && s <= last[1] + 0.05) last[1] = Math.max(last[1], e);
      else out.push([s, e]);
    }
    return out;
  };
  const removeRange = (list, a, b) => {
    const out = [];
    for (const [s, e] of list) {
      if (e <= a || s >= b) out.push([s, e]);
      else {
        if (s < a) out.push([s, a]);
        if (e > b) out.push([b, e]);
      }
    }
    return out;
  };
  const intersect = (lists) => {
    if (!lists.length) return [];
    let out = lists[0];
    for (const other of lists.slice(1)) {
      const next = [];
      for (const [a, b] of out)
        for (const [c, d] of other) {
          const s = Math.max(a, c);
          const e = Math.min(b, d);
          if (e > s) next.push([s, e]);
        }
      out = next;
    }
    return out;
  };

  // --- Fragmented MP4: the times a media segment holds -------------------
  const u32 = (v, o) => v.getUint32(o);
  /** Each box in `v` from `start` to `end`: [type, content start, end]. */
  function* boxes(v, start, end) {
    let o = start;
    while (o + 8 <= end) {
      let size = u32(v, o);
      const type = String.fromCharCode(v.getUint8(o + 4), v.getUint8(o + 5), v.getUint8(o + 6), v.getUint8(o + 7));
      let head = 8;
      if (size === 1) {
        size = Number(v.getBigUint64(o + 8));
        head = 16;
      } else if (size === 0) size = end - o;
      if (size < head || o + size > end) return;
      yield [type, o + head, o + size];
      o += size;
    }
  }
  const find = (v, start, end, path) => {
    let range = [start, end];
    for (const name of path) {
      const hit = [...boxes(v, range[0], range[1])].find(([t]) => t === name);
      if (!hit) return null;
      range = [hit[1], hit[2]];
    }
    return range;
  };
  /** What an init segment says: its timescale, default sample duration, picture size. */
  const readInit = (v) => {
    const moov = find(v, 0, v.byteLength, ["moov"]);
    if (!moov) return null;
    const init = { timescale: 1000, duration: 0, width: 0, height: 0 };
    const mdhd = find(v, moov[0], moov[1], ["trak", "mdia", "mdhd"]);
    if (mdhd) init.timescale = u32(v, mdhd[0] + (v.getUint8(mdhd[0]) === 1 ? 20 : 12));
    const trex = find(v, moov[0], moov[1], ["mvex", "trex"]);
    if (trex) init.duration = u32(v, trex[0] + 12);
    const tkhd = find(v, moov[0], moov[1], ["trak", "tkhd"]);
    if (tkhd) {
      const at = tkhd[1] - 8;
      init.width = u32(v, at) >>> 16;
      init.height = u32(v, at + 4) >>> 16;
    }
    return init;
  };
  /** The stretches (seconds) the media segments in `v` hold, by `init`. */
  const mediaTimes = (v, init) => {
    const out = [];
    for (const [type, s, e] of boxes(v, 0, v.byteLength)) {
      if (type !== "moof") continue;
      for (const [t, ts, te] of boxes(v, s, e)) {
        if (t !== "traf") continue;
        let base = 0;
        let defaultDuration = init.duration;
        let total = 0;
        for (const [b, bs] of boxes(v, ts, te)) {
          const version = v.getUint8(bs);
          const flags = u32(v, bs) & 0xffffff;
          if (b === "tfdt") base = version === 1 ? Number(v.getBigUint64(bs + 4)) : u32(v, bs + 4);
          if (b === "tfhd") {
            let o = bs + 8;
            if (flags & 0x1) o += 8;
            if (flags & 0x2) o += 4;
            if (flags & 0x8) defaultDuration = u32(v, o);
          }
          if (b === "trun") {
            const count = u32(v, bs + 4);
            let o = bs + 8;
            if (flags & 0x1) o += 4;
            if (flags & 0x4) o += 4;
            const each = [0x100, 0x200, 0x400, 0x800].filter((f) => flags & f).length * 4;
            for (let i = 0; i < count; i++) {
              total += flags & 0x100 ? u32(v, o) : defaultDuration;
              o += each;
            }
          }
        }
        if (total > 0) out.push([base / init.timescale, (base + total) / init.timescale]);
      }
    }
    return out;
  };

  // --- Reading segments as they come -----------------------------------
  // An append can end anywhere (YouTube's split boxes and clusters), so each
  // reader keeps what it hasn't finished, as a browser's stream parsers do,
  // and says the stretches of media (seconds) it has seen.
  const join = (a, b) => {
    if (!a.length) return b;
    const out = new Uint8Array(a.length + b.length);
    out.set(a);
    out.set(b, a.length);
    return out;
  };

  /** Fragmented MP4: top-level boxes; moov (the init) and moof read whole, the rest skipped. */
  class Mp4Reader {
    constructor() {
      this.pending = new Uint8Array(0);
      this.skip = 0;
      this.init = null;
    }
    push(bytes) {
      const buf = join(this.pending, bytes);
      const v = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
      const out = [];
      let o = 0;
      while (o < buf.length) {
        if (this.skip > 0) {
          const n = Math.min(this.skip, buf.length - o);
          this.skip -= n;
          o += n;
          continue;
        }
        if (o + 8 > buf.length) break;
        let size = v.getUint32(o);
        const type = String.fromCharCode(buf[o + 4], buf[o + 5], buf[o + 6], buf[o + 7]);
        let head = 8;
        if (size === 1) {
          if (o + 16 > buf.length) break;
          size = Number(v.getBigUint64(o + 8));
          head = 16;
        }
        if (type === "ftyp" || type === "moov") this.sawInit = true;
        if (type !== "moov" && type !== "moof") {
          // mdat and the rest: passed over, however far they reach.
          o += head;
          this.skip = size === 0 ? 0 : size - head;
          continue;
        }
        if (o + size > buf.length) break;
        const box = new DataView(buf.buffer, buf.byteOffset + o, size);
        if (type === "moov") this.init = readInit(box) ?? this.init;
        else if (this.init) out.push(...mediaTimes(box, this.init));
        o += size;
      }
      this.pending = buf.slice(o);
      return out;
    }
  }

  /** An EBML number at `o` in `buf` (an id: with its marker): [value, length],
   *  or null if it runs past the end; an unknown size is -1. */
  const ebml = (buf, o, id) => {
    if (o >= buf.length) return null;
    const first = buf[o];
    let length = 1;
    while (length <= 8 && !(first & (0x80 >> (length - 1)))) length++;
    if (length > 8 || o + length > buf.length) return null;
    let value = id ? first : first & (0xff >> length);
    let allOnes = value === 0xff >> length;
    for (let i = 1; i < length; i++) {
      value = value * 256 + buf[o + i];
      if (buf[o + i] !== 0xff) allOnes = false;
    }
    return [!id && allOnes ? -1 : value, length];
  };

  /** WebM: elements read flat (into the segment, clusters and block groups),
   *  each block's time from its cluster's; media data skipped. A block's
   *  length isn't written: it counts as 20 ms (frames meet across that). */
  class WebmReader {
    constructor() {
      this.pending = new Uint8Array(0);
      this.skip = 0;
      this.scale = 1e-3;
      this.base = null;
      this.init = null;
    }
    push(bytes) {
      const buf = join(this.pending, bytes);
      const out = [];
      let o = 0;
      const uint = (from, to) => {
        let n = 0;
        for (let i = from; i < to; i++) n = n * 256 + buf[i];
        return n;
      };
      while (o < buf.length) {
        if (this.skip > 0) {
          const n = Math.min(this.skip, buf.length - o);
          this.skip -= n;
          o += n;
          continue;
        }
        const id = ebml(buf, o, true);
        if (!id) break;
        const size = ebml(buf, o + id[1], false);
        if (!size) break;
        const head = id[1] + size[1];
        const [name, length] = [id[0], size[0]];
        if (name === 0x1a45dfa3) {
          // The EBML header: an init segment begins.
          this.init = { webm: true };
          this.sawInit = true;
          o += head;
          this.skip = Math.max(0, length);
        } else if (name === 0x18538067 || name === 0x1549a966 || name === 0xa0) {
          // Segment, Info, BlockGroup: into them.
          o += head;
        } else if (name === 0x1f43b675) {
          // A cluster: its timecode comes first.
          this.base = null;
          o += head;
        } else if (name === 0x2ad7b1 || name === 0xe7) {
          if (o + head + length > buf.length) break;
          const n = uint(o + head, o + head + length);
          if (name === 0x2ad7b1) this.scale = n / 1e9;
          else this.base = n;
          o += head + length;
        } else if (name === 0xa3 || name === 0xa1) {
          // A block: its track number, then its time relative to the cluster's.
          const track = ebml(buf, o + head, false);
          if (!track || o + head + track[1] + 2 > buf.length) break;
          const at = o + head + track[1];
          const relative = (((buf[at] << 8) | buf[at + 1]) << 16) >> 16;
          if (this.base !== null) {
            const t = (this.base + relative) * this.scale;
            out.push([t, t + 0.02]);
          }
          o += head;
          this.skip = length;
        } else {
          o += head;
          this.skip = length < 0 ? 0 : length;
        }
      }
      this.pending = buf.slice(o);
      return out;
    }
  }

  // --- SourceBuffer -----------------------------------------------------
  let buffers = 0;
  class StandInSourceBuffer extends EventTarget {
    constructor(source, type) {
      super();
      this.id = ++buffers;
      this.source = source;
      this.type = type;
      this.updating = false;
      this.mode = "segments";
      this.timestampOffset = 0;
      this.appendWindowStart = 0;
      this.appendWindowEnd = Infinity;
      this.held = [];
      this.init = null;
      this.reader = /webm/.test(type) ? new WebmReader() : new Mp4Reader();
      for (const name of ["updatestart", "update", "updateend", "error", "abort"]) this["on" + name] = null;
    }
    get buffered() {
      return new Ranges(this.held.map((r) => [...r]));
    }
    fire(name) {
      const e = new Event(name);
      this.dispatchEvent(e);
      if (typeof this["on" + name] === "function") this["on" + name](e);
    }
    appendBuffer(data) {
      if (this.updating) throw new DOMException("still updating", "InvalidStateError");
      const view =
        data instanceof ArrayBuffer
          ? new Uint8Array(data)
          : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      const bytes = view.slice();
      this.updating = true;
      this.fire("updatestart");
      setTimeout(() => {
        const ranges = [];
        this.reader.sawInit = false;
        try {
          for (const [a, b] of this.reader.push(bytes)) {
            const s = a + this.timestampOffset;
            const e = b + this.timestampOffset;
            ranges.push([s, e]);
            const from = Math.max(s, this.appendWindowStart);
            const to = Math.min(e, this.appendWindowEnd);
            if (to > from) this.held = addRange(this.held, from, to);
          }
          this.init = this.reader.init;
        } catch (e) {
          // Never left updating: their player would wait on it for good.
          post({ kind: "error", id: this.id, message: String(e) });
        }
        const copy = bytes.slice().buffer;
        post(
          {
            kind: "append",
            id: this.id,
            type: this.type,
            size: copy.byteLength,
            init: this.reader.sawInit,
            ranges,
            // Ends on a whole box or block: the media its ranges name is all here.
            complete: this.reader.pending.length === 0 && this.reader.skip === 0,
            bytes: copy,
          },
          [copy],
        );
        this.updating = false;
        this.fire("update");
        this.fire("updateend");
        this.source.appended();
      }, 0);
    }
    remove(start, end) {
      if (this.updating) throw new DOMException("still updating", "InvalidStateError");
      this.updating = true;
      this.fire("updatestart");
      setTimeout(() => {
        this.held = removeRange(this.held, start, end);
        this.updating = false;
        this.fire("update");
        this.fire("updateend");
      }, 0);
    }
    abort() {
      this.updating = false;
    }
    changeType(type) {
      this.type = type;
      this.reader = /webm/.test(type) ? new WebmReader() : new Mp4Reader();
    }
  }

  // --- MediaSource ------------------------------------------------------
  const standIns = new Map();
  let sources = 0;
  class StandInMediaSource extends EventTarget {
    static isTypeSupported(type) {
      return /avc1|avc3|mp4a|opus|vp09|vp9|av01/.test(type) && !/hev|hvc/.test(type);
    }
    constructor() {
      super();
      this.id = ++sources;
      this._ready = "closed";
      this.list = [];
      this.element = null;
      this._duration = NaN;
      /** A real MediaSource this one hands everything to (an ad's: `handOver`). */
      this.real = null;
      for (const name of ["sourceopen", "sourceended", "sourceclose"]) this["on" + name] = null;
    }
    get readyState() {
      return this.real ? this.real.readyState : this._ready;
    }
    set readyState(state) {
      this._ready = state;
    }
    get sourceBuffers() {
      if (this.real) return this.real.sourceBuffers;
      return Object.assign([...this.list], { onaddsourcebuffer: null, onremovesourcebuffer: null });
    }
    get activeSourceBuffers() {
      return this.sourceBuffers;
    }
    get duration() {
      return this.real ? this.real.duration : this._duration;
    }
    set duration(d) {
      if (this.real) {
        this.real.duration = d;
        return;
      }
      this._duration = d;
      this.element?.media?.durationChanged();
      // Not the VOD (an ad we didn't see coming?): the page falls back to
      // their plain player rather than show it as the VOD.
      if (vodDuration > 0 && Number.isFinite(d) && Math.abs(d - vodDuration) > MISMATCH)
        post({ kind: "mismatch", source: this.id, duration: d, vod: vodDuration });
    }
    fire(name) {
      const e = new Event(name);
      this.dispatchEvent(e);
      if (typeof this["on" + name] === "function") this["on" + name](e);
    }
    addSourceBuffer(type) {
      if (this.real) return this.real.addSourceBuffer(type);
      const buffer = new StandInSourceBuffer(this, type);
      this.list.push(buffer);
      return buffer;
    }
    removeSourceBuffer(buffer) {
      if (this.real) return this.real.removeSourceBuffer(buffer);
      this.list = this.list.filter((b) => b !== buffer);
    }
    endOfStream(error) {
      if (this.real) return error ? this.real.endOfStream(error) : this.real.endOfStream();
      this.readyState = "ended";
      this.fire("sourceended");
      this.element?.media?.update();
    }
    setLiveSeekableRange(start, end) {
      this.real?.setLiveSeekableRange(start, end);
    }
    clearLiveSeekableRange() {
      this.real?.clearLiveSeekableRange();
    }
    appended() {
      this.element?.media?.update();
    }
    get buffered() {
      return intersect(this.list.map((b) => b.held));
    }
  }
  /** How far (seconds) a source's length may be from the VOD's and be the VOD. */
  const MISMATCH = 2;
  const RealMediaSource = window.MediaSource;
  window.MediaSource = StandInMediaSource;
  if (window.ManagedMediaSource) window.ManagedMediaSource = StandInMediaSource;

  const createObjectURL = URL.createObjectURL;
  URL.createObjectURL = function (object) {
    if (object instanceof StandInMediaSource) {
      const url = `blob:${location.origin}/vodscribe-${object.id}`;
      standIns.set(url, object);
      return url;
    }
    return createObjectURL.call(this, object);
  };

  // --- The media element ------------------------------------------------
  /** The state of an element while it plays a stand-in source. */
  class Media {
    constructor(element, source, url) {
      this.element = element;
      this.source = source;
      this.url = url;
      this.time = 0;
      this.paused = true;
      this.seeking = false;
      this.ended = false;
      this.rate = 1;
      this.volume = 1;
      this.muted = false;
      this.readyState = 0;
      this.since = 0;
      this.timer = 0;
      this.lastTimeUpdate = 0;
      /** The app's player's time, as last told ({time, playing, at}). */
      this.told = null;
      /** Till then, what's told is older than what this element was asked. */
      this.quietUntil = 0;
    }
    /** Told by the app's player where it is. */
    tell(clock) {
      this.told = { ...clock, at: performance.now() };
    }
    asked() {
      this.quietUntil = performance.now() + 600;
    }
    fire(name) {
      this.element.dispatchEvent(new Event(name));
    }
    now() {
      // The app's player plays it: its time is the time.
      if (this.told && !this.seeking && performance.now() > this.quietUntil) {
        const { time, playing, at } = this.told;
        // Paused here, it stands, whatever an older word said.
        return playing && !this.paused ? time + ((performance.now() - at) / 1000) * this.rate : time;
      }
      if (this.paused || this.seeking || this.readyState < 3) return this.time;
      return this.time + ((performance.now() - this.since) / 1000) * this.rate;
    }
    /** Where the data runs out ahead of `t`. */
    aheadOf(t) {
      for (const [a, b] of this.source.buffered) if (t >= a - 0.1 && t < b) return b;
      return t;
    }
    durationChanged() {
      this.fire("durationchange");
    }
    setReady(state) {
      const was = this.readyState;
      if (state === was) return;
      this.time = this.now();
      this.since = performance.now();
      this.readyState = state;
      if (was < 1 && state >= 1) this.fire("loadedmetadata");
      if (was < 2 && state >= 2) this.fire("loadeddata");
      if (was < 3 && state >= 3) {
        this.fire("canplay");
        if (!this.paused) this.fire("playing");
      }
      if (was < 4 && state >= 4) this.fire("canplaythrough");
      if (was >= 3 && state < 3 && !this.paused) this.fire("waiting");
    }
    /** Looks again at what's buffered: ready to play here or not; a seek landed. */
    update() {
      const t = this.now();
      const ahead = this.aheadOf(t) - t;
      const end = this.source.readyState === "ended";
      const hasInit = this.source.list.some((b) => b.init);
      let state = hasInit ? 1 : 0;
      if (hasInit && ahead > 0.05) state = 2;
      if (hasInit && (ahead > 0.5 || (end && ahead > 0))) state = 3;
      if (hasInit && (ahead > 3 || (end && ahead > 0))) state = 4;
      if (this.seeking && state >= 2) {
        this.seeking = false;
        this.since = performance.now();
        this.fire("seeked");
        this.fire("timeupdate");
      }
      this.setReady(state);
    }
    tick() {
      if (this.paused) return;
      const t = this.now();
      const duration = this.source.duration;
      if (Number.isFinite(duration) && t >= duration - 0.05 && this.source.readyState === "ended") {
        this.time = duration;
        this.paused = true;
        this.ended = true;
        clearInterval(this.timer);
        this.fire("timeupdate");
        this.fire("pause");
        this.fire("ended");
        return;
      }
      this.update();
      if (performance.now() - this.lastTimeUpdate > 250) {
        this.lastTimeUpdate = performance.now();
        this.fire("timeupdate");
      }
    }
    play() {
      if (this.paused) {
        this.time = this.now();
        this.since = performance.now();
        this.paused = false;
        this.ended = false;
        this.fire("play");
        if (this.readyState >= 3) this.fire("playing");
        else this.fire("waiting");
        clearInterval(this.timer);
        this.timer = setInterval(() => this.tick(), 50);
        this.asked();
        post({ kind: "media", event: "play", time: this.time });
      }
      return Promise.resolve();
    }
    pause() {
      if (!this.paused) {
        this.time = this.now();
        this.paused = true;
        clearInterval(this.timer);
        this.fire("timeupdate");
        this.fire("pause");
        this.asked();
        post({ kind: "media", event: "pause", time: this.time });
      }
    }
    seek(t) {
      this.time = Math.max(0, t);
      this.since = performance.now();
      this.seeking = true;
      this.ended = false;
      this.fire("seeking");
      this.asked();
      post({ kind: "media", event: "seek", time: this.time, paused: this.paused });
      this.update();
    }
  }

  /** The element's own properties `takeOver` defines (all of them, over its runs). */
  const OWN = new Set();

  /** Gives `element` back its own properties and media: what's taken over
   *  of it ends (its stand-in no longer feeds the app's player). */
  const release = (element) => {
    const media = element.media;
    if (!media) return;
    clearInterval(media.timer);
    clearInterval(media.controls);
    // The app's player stops with it (resumed, if so, when the next takes over).
    if (!media.paused) post({ kind: "media", event: "pause", time: media.now() });
    if (playing === media) playing = null;
    if (media.source.element === element) media.source.element = null;
    for (const name of OWN) delete element[name];
    element.media = null;
  };

  /** While an ad shows, their player plays it through a MediaSource of its
   *  own (YouTube: a new one, on the same element): `source` hands all to a
   *  real one, so the element plays the ad itself, over our picture, and
   *  none of it goes to the app's player. */
  const handOver = (element, source) => {
    release(element);
    const real = new RealMediaSource();
    source.real = real;
    for (const name of ["sourceopen", "sourceended", "sourceclose"]) real.addEventListener(name, () => source.fire(name));
    srcDescriptor.set.call(element, createObjectURL.call(URL, real));
    post({ kind: "ad", phase: "handed", source: source.id });
  };

  /** Makes `element` play stand-in `source` (at `url`): its own properties say what Media holds. */
  const takeOver = (element, source, url) => {
    release(element);
    const media = new Media(element, source, url);
    playing = media;
    source.element = element;
    element.media = media;
    const define = (name, get, set) => {
      Object.defineProperty(element, name, { configurable: true, get, ...(set ? { set } : {}) });
      OWN.add(name);
    };
    define(
      "src",
      () => url,
      (value) => setSource(element, value),
    );
    define("currentSrc", () => url);
    define(
      "currentTime",
      () => media.now(),
      (t) => media.seek(Number(t)),
    );
    define("duration", () => (Number.isFinite(source.duration) ? source.duration : NaN));
    define("paused", () => media.paused);
    define("ended", () => media.ended);
    define("seeking", () => media.seeking);
    define("readyState", () => media.readyState);
    define("networkState", () => 2);
    define("buffered", () => new Ranges(source.buffered.map((r) => [...r])));
    define("seekable", () => new Ranges(Number.isFinite(source.duration) ? [[0, source.duration]] : []));
    define("played", () => new Ranges([]));
    define("error", () => null);
    define(
      "playbackRate",
      () => media.rate,
      (r) => {
        media.time = media.now();
        media.since = performance.now();
        media.rate = Number(r);
        media.fire("ratechange");
        post({ kind: "media", event: "rate", rate: media.rate });
      },
    );
    define(
      "defaultPlaybackRate",
      () => 1,
      () => {},
    );
    define(
      "volume",
      () => media.volume,
      (v) => {
        media.volume = Number(v);
        media.fire("volumechange");
        post({ kind: "media", event: "volume", volume: media.volume, muted: media.muted });
      },
    );
    define(
      "muted",
      () => media.muted,
      (m) => {
        media.muted = !!m;
        media.fire("volumechange");
        post({ kind: "media", event: "volume", volume: media.volume, muted: media.muted });
      },
    );
    define("videoWidth", () => source.list.find((b) => b.init?.width)?.init.width ?? 0);
    define("videoHeight", () => source.list.find((b) => b.init?.height)?.init.height ?? 0);
    for (const name of ["play", "pause", "load", "getVideoPlaybackQuality"]) OWN.add(name);
    element.play = () => media.play();
    element.pause = () => media.pause();
    element.load = () => {};
    element.getVideoPlaybackQuality = () => ({
      creationTime: performance.now(),
      totalVideoFrames: Math.floor(media.now() * 30),
      droppedVideoFrames: 0,
      corruptedVideoFrames: 0,
    });
    // Their frame clear behind the video, for our picture under it to show
    // through: each opaque background behind it (their controls and all
    // over it stay as they are).
    const clear = () => {
      for (let el = element.parentElement; el; el = el.parentElement) {
        const bg = getComputedStyle(el).backgroundColor;
        if (bg !== "rgba(0, 0, 0, 0)" && bg !== "transparent")
          el.style.setProperty("background-color", "transparent", "important");
      }
    };
    clear();
    setTimeout(clear, 1000);
    media.fire("loadstart");
    setTimeout(() => {
      source.readyState = "open";
      source.fire("sourceopen");
    }, 0);
    // How far up from the bottom their controls reach while they show
    // (Twitch's fade in and out over the video): the page keeps our captions
    // above them. Read only; theirs are left as they are.
    let lastCover = -1;
    media.controls = setInterval(() => {
      const controls = document.querySelector('[data-a-target="player-controls"]');
      let cover = 0;
      if (controls) {
        let shown = true;
        for (let a = controls; a; a = a.parentElement) {
          const cs = getComputedStyle(a);
          if (cs.visibility === "hidden" || cs.display === "none" || Number(cs.opacity) < 0.1) shown = false;
        }
        if (shown) cover = Math.max(0, Math.round(window.innerHeight - controls.getBoundingClientRect().top));
      }
      if (cover !== lastCover) {
        lastCover = cover;
        post({ kind: "controls", cover });
      }
    }, 250);
  };

  /** The element playing a stand-in now: the app's player's clock goes to it. */
  let playing = null;
  window.addEventListener("message", (e) => {
    if (e.source !== window.parent) return;
    const clock = e.data && e.data.vodscribeClock;
    const vod = e.data && e.data.vodscribeVod;
    if (vod) {
      vodDuration = Number(vod.duration) || 0;
      return;
    }
    if (e.data && typeof e.data.vodscribeAd === "boolean") {
      adTold = e.data.vodscribeAd;
      return;
    }
    if (clock && playing) playing.tell(clock);
  });
  const proto = HTMLMediaElement.prototype;
  const srcDescriptor = Object.getOwnPropertyDescriptor(proto, "src");
  const setSource = (element, value) => {
    const source = standIns.get(String(value));
    if (source && adShowing() && RealMediaSource) handOver(element, source);
    else if (source) takeOver(element, source, String(value));
    else {
      release(element);
      srcDescriptor.set.call(element, value);
    }
  };
  Object.defineProperty(proto, "src", {
    configurable: true,
    get: srcDescriptor.get,
    set(value) {
      setSource(this, value);
    },
  });
  const setAttribute = Element.prototype.setAttribute;
  Element.prototype.setAttribute = function (name, value) {
    if (this instanceof HTMLMediaElement && String(name).toLowerCase() === "src" && standIns.has(String(value))) {
      return setSource(this, value);
    }
    return setAttribute.call(this, name, value);
  };
})();
