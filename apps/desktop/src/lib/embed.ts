// Controllers for the platforms' players. YouTube and Twitch run in sandboxed
// iframes on their own origins and are driven over postMessage, using the same
// messages their embed scripts send, so no third-party script runs in the app's
// window (where it could reach the app's commands). They still give a real
// seek(), so jumping between timestamps doesn't reload the player.
//
// In a build with the embed-takeover feature (off in releases: the sites'
// terms forbid it), where the app's own player can draw, a YouTube or Twitch
// VOD's picture is the app's, under their player (bridge): their player stays
// on top, with its controls, ads and all, and what it fetches the app's
// player plays, frame-exact (embed_shim.js in their frame, embed.rs).
// Otherwise their player plays it alone, as served.

import { ownPlayer } from "./ownPlayer.svelte";
import { cueAt, elementSound, unplayable } from "./media";
import { landingSeeks } from "./mediaSeek";
import { OwnStatus } from "./ownStatus";
import { settling } from "./settle";
import { Clock } from "./playerClock";
import { mount, unmount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import Screen from "./Screen.svelte";
import { api, events, type Cue, type Vod } from "./api";
import * as overlay from "./screenOverlay";
import { prefs } from "./playerPrefs.svelte";
import LocalPlayer from "./LocalPlayer.svelte";
import NativePlayer from "./NativePlayer.svelte";
import { isTwitchClip, twitchId, twitchTime } from "./platform";

export interface EmbeddedPlayer {
  seek(seconds: number, play: boolean): void;
  /** Current position in seconds. */
  time(): number;
  /** Whether it's playing (or about to: buffering). */
  playing(): boolean;
  togglePlay(): void;
  /** Steps `frames` frames on (back, if negative), paused. Only where frames can be told apart. */
  step?(frames: number): void;
  /** How long it is, where the player knows (it can differ from what the site said). */
  duration?(): number;
  /** Its frame rate, where known exactly (the app's own player). */
  fps?(): number | null;
  /** Where the frame on screen is, where the player knows (the app's own;
   *  null: none shows). The playhead follows it; `time` may run ahead. */
  shown?(): number | null;
  /** When its last seek's reply came (performance.now()), null while one is
   *  out (the app's own player: playerReading.svelte.ts). */
  settled?(): number | null;
  /** Its sound, where the app's controls set it (0..1). */
  volume?: { get(): number; muted(): boolean; set(volume: number): void; setMuted(muted: boolean): void };
  /** Its speed, where the app's controls set it. */
  rate?: { get(): number; set(rate: number): void };
  /** Captions from the transcript, where it shows them (the app's own player, or its picture under a site's);
   *  `none`: it would, but there are none (nothing transcribed yet). */
  captions?: { available(): boolean; on(): boolean; toggle(): void; none?(): boolean };
  /** Shows controls of its own that can't be hidden (Twitch's), so the app's leave out sound and speed. */
  ownControls?: boolean;
  destroy(): void;
}

/** A frame for a site's player; `plain`, the script the app puts in every
 *  frame (embed_shim.js) leaves it alone: the site's player plays by itself. */
function sandboxedFrame(el: HTMLElement, src: string, plain = true): HTMLIFrameElement {
  const frame = document.createElement("iframe");
  if (plain) frame.name = PLAIN;
  // Scripts run on the player's own origin; it can't navigate the app or open windows.
  frame.setAttribute("sandbox", "allow-scripts allow-same-origin allow-presentation");
  frame.setAttribute("allow", "autoplay; fullscreen; encrypted-media; picture-in-picture");
  frame.setAttribute("allowfullscreen", "");
  frame.referrerPolicy = "strict-origin-when-cross-origin";
  // A block, not inline: an inline frame sits on a text line, whose descender
  // space pushes it past the bottom of its box.
  frame.style.cssText = "display: block; width: 100%; height: 100%; border: 0";
  frame.src = src;
  el.appendChild(frame);
  return frame;
}

/**
 * Listens for messages from `frame` (and only it), and resolves with `player`
 * once `isReady` sees a ready message, or after a timeout so commands still go
 * through if the ready message never comes.
 */
function connect(
  frame: HTMLIFrameElement,
  origin: string,
  onMessage: (data: unknown) => boolean,
  player: (cleanup: () => void) => EmbeddedPlayer,
  onStart?: () => () => void,
): Promise<EmbeddedPlayer> {
  return new Promise((resolve) => {
    let done = false;
    const stopStart = onStart?.();
    const finish = () => {
      if (done) return;
      done = true;
      clearTimeout(timeout);
      stopStart?.();
      resolve(p);
    };
    const listener = (e: MessageEvent) => {
      if (e.source !== frame.contentWindow || e.origin !== origin) return;
      if (onMessage(e.data)) finish();
    };
    const timeout = setTimeout(finish, 10_000);
    window.addEventListener("message", listener);
    const p = player(() => {
      window.removeEventListener("message", listener);
      clearTimeout(timeout);
      stopStart?.();
      frame.remove();
    });
  });
}

const YOUTUBE = "https://www.youtube-nocookie.com";

async function youtube(el: HTMLElement, vod: Vod, time: number, play: boolean): Promise<EmbeddedPlayer> {
  const ours = await bridgeable(vod);
  const params = new URLSearchParams({
    enablejsapi: "1",
    origin: location.origin,
    start: String(Math.floor(time)),
    autoplay: play ? "1" : "0",
    playsinline: "1",
    rel: "0",
    // The app's controls and timeline stand in for the player's own.
    controls: "0",
    disablekb: "1",
    fs: "0",
  });
  const embedUrl = () => `${YOUTUBE}/embed/${encodeURIComponent(vod.platform_id)}?${params}`;
  const frame = sandboxedFrame(el, embedUrl(), !ours);
  // Their plain player (fallen back, or never ours): its own captions and all.
  let plain = !ours;
  const bridged = ours
    ? bridge(
        el,
        frame,
        YOUTUBE,
        vod,
        time,
        (at, playing) => {
          plain = true;
          params.set("start", String(Math.floor(at)));
          params.set("autoplay", playing ? "1" : "0");
          reloadPlain(frame, embedUrl());
          // It reports to a page that says it's listening, again.
          const timer = setInterval(() => post({ event: "listening" }), 250);
          setTimeout(() => clearInterval(timer), 10_000);
        },
        (at) => command("seekTo", [at, true]),
      )
    : null;
  const clock = new Clock(time, play, { waitsForPlayer: true });
  const sound = { volume: 1, muted: false, rate: 1 };
  const post = (message: object) =>
    frame.contentWindow?.postMessage(JSON.stringify({ ...message, id: 1, channel: "widget" }), YOUTUBE);
  const command = (func: string, args: unknown[] = []) => post({ event: "command", func, args });
  let captionsOff = false;

  return connect(
    frame,
    YOUTUBE,
    (raw) => {
      let data: {
        event?: string;
        info?: {
          currentTime?: number;
          playerState?: number;
          volume?: number;
          muted?: boolean;
          playbackRate?: number;
          namespaces?: string[];
        };
      };
      try {
        data = typeof raw === "string" ? JSON.parse(raw) : (raw as typeof data);
      } catch {
        return false;
      }
      // State 1 is "playing", 3 is "buffering" (moving soon, not yet).
      const state = data.info?.playerState;
      const moving = typeof state === "number" ? state === 1 : undefined;
      heard(clock, data.info?.currentTime, moving, typeof state === "number" ? [1, 3].includes(state) : undefined);
      if (typeof data.info?.volume === "number") sound.volume = data.info.volume / 100;
      if (typeof data.info?.muted === "boolean") sound.muted = data.info.muted;
      if (typeof data.info?.playbackRate === "number") {
        sound.rate = data.info.playbackRate;
        clock.setSpeed(sound.rate);
      }
      // Their ads: told to us once we ask (addEventListener through their API).
      if (data.event === "onReady" && bridged) {
        command("addEventListener", ["onAdStart"]);
        command("addEventListener", ["onAdEnd"]);
      }
      if (data.event === "onAdStart") bridged?.adStarted();
      if (data.event === "onAdEnd") bridged?.adEnded();
      // Our captions show over our picture (bridge), so theirs are turned
      // off, through their player's own API, once, when it says its captions
      // module has loaded (apiInfoDelivery naming it). Turned on again in
      // their player, they stay.
      if (data.event === "apiInfoDelivery" && data.info?.namespaces?.includes("captions") && !captionsOff && !plain) {
        captionsOff = true;
        command("setOption", ["captions", "track", {}]);
        command("unloadModule", ["captions"]);
      }
      return data.event === "onReady" || data.event === "initialDelivery";
    },
    (cleanup) => ({
      ...clocked(
        clock,
        {
          seek: (seconds) => {
            bridged?.asked();
            command("seekTo", [seconds, true]);
          },
          play: () => command("playVideo"),
          pause: () => command("pauseVideo"),
        },
        bridged,
      ),
      volume: {
        get: () => sound.volume,
        muted: () => sound.muted,
        set(volume) {
          sound.volume = volume;
          command("setVolume", [Math.round(volume * 100)]);
          if (sound.muted && volume > 0) this.setMuted(false);
        },
        setMuted(muted) {
          sound.muted = muted;
          command(muted ? "mute" : "unMute");
        },
      },
      rate: {
        get: () => sound.rate,
        set(rate) {
          sound.rate = rate;
          clock.setSpeed(rate);
          command("setPlaybackRate", [rate]);
        },
      },
      captions: bridged?.captions,
      destroy: () => {
        bridged?.stop();
        cleanup();
      },
    }),
    // The player only reports to a page that has said it's listening.
    () => {
      const timer = setInterval(() => post({ event: "listening" }), 250);
      return () => clearInterval(timer);
    },
  );
}

/** What a site's player reported, told to its clock: where it is (if it
 *  said), whether it's moving, and whether it's playing (buffering counts). */
function heard(clock: Clock, at: unknown, moving: boolean | undefined, playing: boolean | undefined) {
  // The state first: a position said with a change of it is judged as after
  // the change (taken exactly, not as agreeing with where it ran to).
  if (playing !== undefined) clock.playing = playing;
  if (typeof at === "number") clock.heardAt(at, moving);
  else if (moving !== undefined) clock.heardMoving(moving);
}

/** What a site's player answers from its clock, given how to tell it to
 *  seek, play and pause (it says where it is only now and then). Where the
 *  app's player draws its picture (`bridged`), the frame shown and the frame
 *  rate are that player's, exactly; else as near as the site's reports say. */
function clocked(
  clock: Clock,
  tell: { seek(seconds: number): void; play(): void; pause(): void },
  bridged: ReturnType<typeof bridge> | null,
) {
  return {
    seek(seconds: number, play: boolean) {
      tell.seek(seconds);
      if (play) tell.play();
      clock.asked(seconds, play);
    },
    time: () => clock.now(),
    shown: () => (bridged?.drawing() ? bridged.own.shown() : clock.shown()),
    fps: () => (bridged?.drawing() ? bridged.own.fps() : null),
    playing: () => clock.playing,
    togglePlay() {
      if (clock.playing) tell.pause();
      else tell.play();
      clock.set(clock.now());
      clock.playing = !clock.playing;
    },
  };
}

const TWITCH = "https://player.twitch.tv";
// From Twitch's embed script (player.twitch.tv/js/embed/v1.js).
const TWITCH_PROXY = "twitch-embed-player-proxy";
const TWITCH_PAUSE = 2;
const TWITCH_PLAY = 3;
const TWITCH_SEEK = 4;

async function twitch(
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
): Promise<EmbeddedPlayer> {
  if (isTwitchClip(vod)) return twitchClip(el, vod, time, play, onerror);
  const params = new URLSearchParams({
    video: twitchId(vod),
    // Twitch refuses to play unless `parent` names the embedding page's host.
    parent: location.hostname || "localhost",
    referrer: location.href,
    time: twitchTime(time),
    autoplay: String(play),
  });
  const ours = await bridgeable(vod);
  const frame = sandboxedFrame(el, `${TWITCH}/?${params}`, !ours);
  const bridged = ours
    ? bridge(
        el,
        frame,
        TWITCH,
        vod,
        time,
        (at, playing) => {
          params.set("time", twitchTime(at));
          params.set("autoplay", String(playing));
          reloadPlain(frame, `${TWITCH}/?${params}`);
          resumeAt = at;
        },
        (at) => command(TWITCH_SEEK, at),
      )
    : null;
  const clock = new Clock(time, play, { waitsForPlayer: true });
  // Where their plain player, reloaded, is to go on from (bridge), till it does.
  let resumeAt: number | null = null;
  const command = (eventName: number, params: unknown = null) =>
    frame.contentWindow?.postMessage({ eventName, params, namespace: TWITCH_PROXY }, TWITCH);

  return connect(
    frame,
    TWITCH,
    (raw) => {
      const data = raw as {
        namespace?: string;
        eventName?: string;
        params?: { currentTime?: number; playback?: string };
      };
      if (data?.namespace === TWITCH_PROXY && data.eventName === "UPDATE_STATE") {
        const playback = data.params?.playback;
        // Reloaded plain (bridge), it now and then starts from the start
        // whatever its URL said (seen in macOS's WebKit): sent where it was, once.
        const now = data.params?.currentTime;
        if (resumeAt !== null && playback === "Playing" && typeof now === "number") {
          if (Math.abs(now - resumeAt) > 3) command(TWITCH_SEEK, resumeAt);
          resumeAt = null;
        }
        const moving = playback ? playback === "Playing" : undefined;
        heard(
          clock,
          data.params?.currentTime,
          moving,
          playback ? ["Playing", "Buffering"].includes(playback) : undefined,
        );
        return true;
      }
      return data?.namespace === "twitch-embed" && (data.eventName === "ready" || data.eventName === "video.ready");
    },
    (cleanup) => ({
      ...clocked(
        clock,
        {
          seek: (seconds) => {
            bridged?.asked();
            // Sent elsewhere: where a plain reload was to go on from no longer counts.
            resumeAt = null;
            command(TWITCH_SEEK, seconds);
          },
          play: () => command(TWITCH_PLAY),
          pause: () => command(TWITCH_PAUSE),
        },
        bridged,
      ),
      ownControls: true,
      captions: bridged?.captions,
      destroy: () => {
        bridged?.stop();
        cleanup();
      },
    }),
  );
}

/** A plain <video> in `el`, under the app's controls, for streams from a site
 *  (Kick, Twitch clips). `attach` points it at its source and returns any cleanup. */
function videoPlayer(
  el: HTMLElement,
  time: number,
  play: boolean,
  attach: (video: HTMLMediaElement) => (() => void) | void,
  onerror?: (message: string | null) => void,
): EmbeddedPlayer {
  const video = document.createElement("video");
  video.playsInline = true;
  // A click on the picture plays or pauses, as on the app's own player.
  video.addEventListener("click", () => (video.paused ? video.play().catch(() => {}) : video.pause()));
  video.style.cssText = "display: block; width: 100%; height: 100%; background: #000";
  video.addEventListener("error", () => {
    onerror?.(unplayable(video.error));
  });
  // A later successful play (after a retry or seek) clears an earlier error.
  video.addEventListener("playing", () => onerror?.(null));
  el.appendChild(video);
  const detach = attach(video);
  const seeks = landingSeeks(video);
  // Where it was before a seek: the element says it's at the new place at
  // once, before it has the picture there.
  let before: number | null = null;
  const seekTo = (seconds: number) => {
    before = seeks.wanted() === null && !video.seeking ? video.currentTime : before;
    seeks.seek(seconds);
  };
  if (time > 0) seekTo(time);
  if (play) video.play().catch(() => {});
  return {
    seek(seconds, play) {
      seekTo(seconds);
      if (play) video.play().catch(() => {});
    },
    time: () => seeks.wanted() ?? video.currentTime,
    shown: () => (seeks.wanted() !== null || video.seeking ? before : video.currentTime),
    playing: () => !video.paused,
    togglePlay() {
      if (video.paused) video.play().catch(() => {});
      else video.pause();
    },
    duration: () => (Number.isFinite(video.duration) ? video.duration : 0),
    volume: elementSound(() => video),
    rate: { get: () => video.playbackRate, set: (rate) => (video.playbackRate = rate) },
    destroy() {
      detach?.();
      video.remove();
    },
  };
}

/** Kick has no VOD embed, so play its HLS stream in a plain <video>. */
async function kick(
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
): Promise<EmbeddedPlayer> {
  const variants = await api.kickStream(vod.url);
  // 720p is plenty for a preview pane and keeps bandwidth down.
  const pick = variants.filter((v) => v.height <= 720).sort((a, b) => b.height - a.height)[0] ?? variants[0];
  if (!pick) throw new Error("Kick didn't return a playable stream for this VOD.");
  // Loaded only when a Kick VOD plays, which keeps it out of the website's demo.
  const { default: Hls } = await import("hls.js");
  return videoPlayer(
    el,
    time,
    play,
    (video) => {
      if (!Hls.isSupported()) {
        video.src = pick.url;
        video.currentTime = time;
        return;
      }
      const hls = new Hls({ startPosition: time });
      hls.loadSource(pick.url);
      hls.attachMedia(video);
      return () => hls.destroy();
    },
    onerror,
  );
}

/** A Twitch clip: its video, in a plain <video>, which (unlike Twitch's clip
 *  embed) can be told where to play from. */
async function twitchClip(
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
): Promise<EmbeddedPlayer> {
  const src = await api.twitchClipVideo(vod.platform_id);
  return videoPlayer(
    el,
    time,
    play,
    (video) => {
      video.src = src;
      video.currentTime = time;
    },
    onerror,
  );
}

/** A file on this computer in the app's own player (NativePlayer.svelte); or
 *  where that can't draw yet, or the file is sound alone, streamed by the
 *  app's local media server into the webview's (LocalPlayer.svelte). */
async function local(
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
): Promise<EmbeddedPlayer> {
  if (!vod.audio_only && (await ownPlayer.ask())) {
    const player = mount(NativePlayer, { target: el, props: { vod, start: time, autoplay: play, onerror } });
    return {
      seek: player.seek,
      time: player.currentTime,
      playing: player.isPlaying,
      togglePlay: player.togglePlay,
      step: player.step,
      duration: player.mediaDuration,
      fps: player.frameRate,
      shown: player.shownTime,
      settled: player.seekSettled,
      volume: player.sound,
      rate: player.speed,
      captions: player.captions,
      destroy: () => unmount(player),
    };
  }
  const src = await api.mediaUrl(vod.id);
  const player = mount(LocalPlayer, {
    target: el,
    props: { vod, src, start: time, autoplay: play, audio: vod.audio_only, onerror },
  });
  return {
    seek: player.seek,
    time: player.currentTime,
    playing: player.isPlaying,
    togglePlay: player.togglePlay,
    step: player.step,
    duration: player.mediaDuration,
    fps: player.frameRate,
    volume: player.sound,
    rate: player.speed,
    captions: player.captions,
    destroy: () => unmount(player),
  };
}

type CreatePlayer = (
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
) => Promise<EmbeddedPlayer>;

const players: Record<string, CreatePlayer> = {
  youtube,
  twitch,
  kick,
  local,
};

/** A kept video, played like a local file; a VOD from a site, in the
 *  platform's player (with the app's picture under it, where it can). */
function playerFor(vod: Vod): CreatePlayer | undefined {
  if (vod.media) return local;
  return Object.hasOwn(players, vod.platform) ? players[vod.platform] : undefined;
}

/** The name a site's frame gets when its player is to play by itself:
 *  embed_shim.js, in every frame, then leaves it alone. */
const PLAIN = "vodscribe-plain";

/** Loads `url` in `frame` as their plain player (`PLAIN`), by way of a
 *  blank page: a URL that differs only after its "#" wouldn't load anew. */
function reloadPlain(frame: HTMLIFrameElement, url: string) {
  frame.name = PLAIN;
  frame.addEventListener("load", () => (frame.src = `${url}#${PLAIN}`), { once: true });
  frame.src = "about:blank";
}

/** Whether VOD `vod`'s picture can be the app's, under its site's player:
 *  the app's player draws here, and it's a picture (not sound alone) of a
 *  known length (a stream still going on may not say yet). */
async function bridgeable(vod: Vod): Promise<boolean> {
  return !vod.audio_only && (vod.duration ?? 0) > 0 && (await takeover()) && (await ownPlayer.ask());
}

/** Whether this build takes a site's VOD's picture into the app's player at
 *  all (embed.rs embed_takeover; off in releases, for the sites' terms). */
let asked: Promise<boolean> | null = null;
const takeover = () => (asked ??= invoke<boolean>("embed_takeover").catch(() => false));

/** Appends waiting to reach the app at most: past it, it can't keep up. */
const MOST_PENDING = 400;

// What the frames' script reported lately, for the tests
// (tests/e2e/scenarios/site-embed.mjs; not its appends).
const reports: unknown[] = [];
(window as unknown as { __vodscribeEmbed: { reports: unknown[] } }).__vodscribeEmbed = { reports };
const report = (m: object) => {
  reports.push(m);
  if (reports.length > 300) reports.splice(0, reports.length - 300);
};

/** What the site's player in `frame` (at `origin`) appends goes to the app's
 *  player (embed.rs), which shows it under the page in the frame's place;
 *  their player's play, pause, seeks, sound and speed drive it, and its time
 *  is told back to the frame (embed_shim.js). An ad their player plays
 *  itself; what can't be played as the VOD has `reload` bring their plain
 *  player back where it was. */
function bridge(
  el: HTMLElement,
  frame: HTMLIFrameElement,
  origin: string,
  vod: Vod,
  time: number,
  reload: (at: number, playing: boolean) => void,
  seekTheirs: (at: number) => void,
): {
  stop: () => void;
  /** What the app's player last said of itself (the frame shown, its rate). */
  own: OwnStatus;
  /** Whether the app's player draws the picture still (not fallen back). */
  drawing: () => boolean;
  /** The app's controls asked their player to seek. */
  asked: () => void;
  captions: NonNullable<EmbeddedPlayer["captions"]>;
  adStarted: () => void;
  adEnded: () => void;
} {
  let stopped = false;
  // When an ad's source was last handed to their element (embed_shim.js), so
  // their player plays it itself.
  let handedAt = -Infinity;
  // Whether the app's player plays, as it last said.
  let playing = false;
  // When their player last resumed, and when the app's controls last asked it to seek.
  let resumedAt = -Infinity;
  let askedAt = -Infinity;
  /** Twitch's player, resumed after a pause, now and then seeks itself to
   *  the very start while what it stands in for is elsewhere (never by
   *  itself, alone). A seek to the start just after resuming, not asked
   *  for, from well into the VOD and not from its end, is that. */
  const spurious = (to: number) =>
    to < 0.5 &&
    at > 2 &&
    performance.now() - resumedAt < 2000 &&
    performance.now() - askedAt > 2000 &&
    // (Their stream may end a little before the VOD's length says.)
    !(vod.duration && at > vod.duration - 3);
  el.style.position = "relative";
  // Our screen's box goes under their frame, never over it: their player stays
  // on top, and the picture shows through it from under the page.
  el.style.isolation = "isolate";
  const host = document.createElement("div");
  host.style.cssText = "position: absolute; inset: 0; pointer-events: none; z-index: -1";
  el.insertBefore(host, el.firstChild);
  // Light till their document says (a site's player that says nothing is light).
  frame.style.colorScheme = "light";
  const screen = mount(Screen, { target: host, props: { under: true } });
  // The session embed.rs keeps for this VOD: its token goes with each append and the close.
  let token: number | null = null;
  const opened = invoke<number>("embed_open", { vodId: vod.id, at: time }).then(
    (t) => void (token = t),
    (e) => {
      console.warn("embed_open", e);
      fallback("the app's player didn't open");
    },
  );
  // Their element played or paused, passed on to the app's player once it
  // stands (settle.ts: Twitch plays and pauses at once on a user's play);
  // reported, for the tests.
  const playOrPause = settling((play) => {
    report({ kind: "told", playing: play });
    (play ? api.playerPlay() : api.playerPause()).catch(() => {});
  });
  // Appends go one at a time, in the order they came: each follows on from
  // the last. Too many waiting, and the app can't keep up: their player alone.
  let appends: Promise<unknown> = opened;
  let pending = 0;
  const listener = (e: MessageEvent) => {
    if (e.source !== frame.contentWindow || e.origin !== origin) return;
    const m = (
      e.data as {
        vodscribeShim?: {
          kind: string;
          event?: string;
          time?: number;
          paused?: boolean;
          volume?: number;
          muted?: boolean;
          rate?: number;
          id?: number;
          type?: string;
          init?: boolean;
          ranges?: [number, number][];
          complete?: boolean;
          bytes?: ArrayBuffer;
          cover?: number;
          scheme?: string;
        };
      } | null
    )?.vodscribeShim;
    if (!m || stopped) return;
    if (m.kind !== "append") report(m);
    if (m.kind === "loaded") {
      frame.contentWindow?.postMessage({ vodscribeVod: { duration: vod.duration ?? 0 } }, origin);
      return;
    }
    if (m.kind === "error") console.warn("embed frame", m);
    if (m.kind === "ad") {
      // Their element plays the ad, over our picture; ours waits for the VOD's next source.
      handedAt = performance.now();
      playOrPause.stop();
      api.playerPause().catch(() => {});
      return;
    }
    if (m.kind === "mismatch") return fallback("a source that isn't the VOD's");
    if (m.kind === "scheme" && typeof m.scheme === "string") {
      // Their frame in their document's own colour scheme: one that differs
      // is drawn on an opaque background (a dark app, a light player: grey).
      frame.style.colorScheme = m.scheme === "normal" ? "light" : m.scheme;
      return;
    }
    if (m.kind === "controls") {
      // Their controls over the bottom of the picture: our captions above them.
      controlsCover = m.cover ?? 0;
      return;
    }
    void opened.then(() => {
      if (stopped) return;
      if (m.kind === "append" && m.bytes) {
        const bytes = m.bytes;
        if (++pending > MOST_PENDING) return fallback("appends piling up");
        appends = appends.then(() => {
          pending--;
          if (stopped || token === null) return;
          return invoke("embed_append", bytes, {
            headers: {
              "x-session": String(token),
              "x-buffer": String(m.id ?? 0),
              "x-type": m.type ?? "",
              "x-init": m.init ? "1" : "0",
              "x-ranges": JSON.stringify(m.ranges ?? []),
              "x-complete": m.complete ? "1" : "0",
            },
          }).catch((e) => {
            console.warn("embed_append", e);
            report({ kind: "appendError", message: String(e) });
          });
        });
      } else if (m.kind === "media" && m.event === "play") {
        resumedAt = performance.now();
        playOrPause.want(true);
      } else if (m.kind === "media" && m.event === "pause") playOrPause.want(false);
      else if (m.kind === "media" && m.event === "seek" && typeof m.time === "number") {
        // Their player sent back where it was, as a viewer would.
        if (spurious(m.time)) {
          report({ kind: "restored", from: m.time, to: at });
          seekTheirs(at);
          return;
        }
        api.playerSeek(m.time, m.paused === false).catch(() => {});
      }
      // The sound is the app's player's: their element is silent.
      else if (m.kind === "media" && m.event === "volume")
        api.playerSetVolume(m.muted ? 0 : (m.volume ?? 1)).catch(() => {});
      else if (m.kind === "media" && m.event === "rate" && typeof m.rate === "number")
        api.playerSetSpeed(m.rate).catch(() => {});
    });
  };
  window.addEventListener("message", listener);
  // Statuses can arrive out of order (a command's reply and the events
  // travel apart): only a newer one is told, or a late "playing" would
  // undo a pause.
  const own = new OwnStatus();
  // Our captions, drawn over our picture (under their frame, as it is): the
  // cue at the player's time, again whenever it, the box or full screen changes.
  // Asked again as the VOD changes: a VOD just added gets its transcript
  // (so its captions) after it opens.
  let cues: Cue[] = [];
  const fetchCues = () =>
    api.captions(vod.id).then(
      (c) => !stopped && (cues = c),
      (e) => console.error("captions", e),
    );
  void fetchCues();
  const changed = events.vod((v) => v.id === vod.id && void fetchCues());
  let at = time;
  let drawn = "";
  let controlsCover = 0;
  const caption = () => {
    if (stopped) return;
    const r = host.getBoundingClientRect();
    const fullscreen = !!host.closest(".fullscreen");
    const text = prefs.captions && r.width && r.height ? (cueAt(cues, at)?.text ?? null) : null;
    const key = JSON.stringify([text, r.width, r.height, fullscreen, controlsCover]);
    if (key === drawn) return;
    drawn = key;
    overlay.show(text ? overlay.caption(text, r.width, r.height, fullscreen, controlsCover) : null);
  };
  let tick = requestAnimationFrame(function each() {
    // Their frame gone (the page moved on while it was still loading): so is this.
    if (!frame.isConnected) return stop();
    caption();
    tick = requestAnimationFrame(each);
  });
  const told = events.playerTime((status) => {
    if (!own.take(status)) return;
    at = status.time;
    playing = status.playing;
    frame.contentWindow?.postMessage(
      { vodscribeClock: { time: status.time, playing: status.playing && status.running && !status.waiting } },
      origin,
    );
  });
  const stop = () => {
    if (stopped) return;
    stopped = true;
    playOrPause.stop();
    cancelAnimationFrame(tick);
    overlay.show(null);
    window.removeEventListener("message", listener);
    void told.then((unlisten) => unlisten());
    void changed.then((unlisten) => unlisten());
    unmount(screen);
    host.remove();
    // Only this session's: a later VOD's is left be.
    void opened.then(() => token !== null && invoke("embed_close", { token }));
    if (hooks.__vodscribeFallback === hook) delete hooks.__vodscribeFallback;
  };
  // Something of theirs we can't play as the VOD (an ad in its stream, a
  // source of another length): their plain player from here, reloaded where
  // it was, and nothing more of ours this time (no picture, no keeping).
  const fallback = (reason: string) => {
    if (stopped) return;
    report({ kind: "fallback", reason });
    const [where, was] = [at, playing];
    stop();
    reload(where, was);
  };
  // For the tests (tests/e2e/scenarios/site-embed.mjs): what an ad in the stream would do.
  const hooks = window as unknown as { __vodscribeFallback?: () => void };
  const hook = () => fallback("asked (test)");
  hooks.__vodscribeFallback = hook;
  return {
    stop,
    own,
    drawing: () => !stopped,
    asked: () => (askedAt = performance.now()),
    // An ad began (their player said): the frame hands new sources to their
    // element. If none comes, the ad is in the stream itself: fall back.
    adStarted: () => {
      frame.contentWindow?.postMessage({ vodscribeAd: true }, origin);
      const began = performance.now();
      setTimeout(() => {
        if (handedAt < began - 5000) fallback("an ad in the stream");
      }, 3000);
    },
    adEnded: () => frame.contentWindow?.postMessage({ vodscribeAd: false }, origin),
    // None once it's stopped (their plain player, its own captions).
    captions: {
      available: () => !stopped && cues.length > 0,
      none: () => !stopped && cues.length === 0,
      on: () => prefs.captions,
      toggle: () => prefs.toggleCaptions(),
    },
  };
}

/** Creates a player inside `el`, or returns null if there's nothing to play here. */
export function createPlayer(
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
): Promise<EmbeddedPlayer> | null {
  return playerFor(vod)?.(el, vod, time, play, onerror) ?? null;
}

export const canEmbed = (vod: Vod) => playerFor(vod) !== undefined;

/** Whether `vod` plays in the app's own player alone: a file here. */
export const playsHere = (vod: Vod) => playerFor(vod) === local;
