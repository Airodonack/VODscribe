// Controllers for the platforms' players. YouTube and Twitch run in sandboxed
// iframes on their own origins and are driven over postMessage, using the same
// messages their embed scripts send, so no third-party script runs in the app's
// window (where it could reach the app's commands). They still give a real
// seek(), so jumping between timestamps doesn't reload the player.

import { landingSeeks } from "./mediaSeek";
import { Clock } from "./playerClock";
import { mount, unmount } from "svelte";
import { api, type Vod } from "./api";
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
  /** Its sound, where the app's controls set it (0..1). */
  volume?: { get(): number; muted(): boolean; set(volume: number): void; setMuted(muted: boolean): void };
  /** Its speed, where the app's controls set it. */
  rate?: { get(): number; set(rate: number): void };
  /** Captions from the transcript, where it shows them (the app's own player). */
  captions?: { available(): boolean; on(): boolean; toggle(): void };
  /** Shows controls of its own that can't be hidden (Twitch's), so the app's leave out sound and speed. */
  ownControls?: boolean;
  destroy(): void;
}

function sandboxedFrame(el: HTMLElement, src: string): HTMLIFrameElement {
  const frame = document.createElement("iframe");
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

function youtube(el: HTMLElement, vod: Vod, time: number, play: boolean): Promise<EmbeddedPlayer> {
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
  const frame = sandboxedFrame(el, `${YOUTUBE}/embed/${encodeURIComponent(vod.platform_id)}?${params}`);
  const clock = new Clock(time, play);
  const sound = { volume: 1, muted: false, rate: 1 };
  const post = (message: object) =>
    frame.contentWindow?.postMessage(JSON.stringify({ ...message, id: 1, channel: "widget" }), YOUTUBE);
  const command = (func: string, args: unknown[] = []) => post({ event: "command", func, args });

  return connect(
    frame,
    YOUTUBE,
    (raw) => {
      let data: {
        event?: string;
        info?: { currentTime?: number; playerState?: number; volume?: number; muted?: boolean; playbackRate?: number };
      };
      try {
        data = typeof raw === "string" ? JSON.parse(raw) : (raw as typeof data);
      } catch {
        return false;
      }
      // State 1 is "playing", 3 is "buffering" (moving soon, not yet).
      const state = data.info?.playerState;
      const moving = typeof state === "number" ? state === 1 : undefined;
      if (typeof data.info?.currentTime === "number") clock.heardAt(data.info.currentTime, moving);
      else if (moving !== undefined) clock.heardMoving(moving);
      if (typeof state === "number") clock.playing = [1, 3].includes(state);
      if (typeof data.info?.volume === "number") sound.volume = data.info.volume / 100;
      if (typeof data.info?.muted === "boolean") sound.muted = data.info.muted;
      if (typeof data.info?.playbackRate === "number") sound.rate = data.info.playbackRate;
      return data.event === "onReady" || data.event === "initialDelivery";
    },
    (cleanup) => ({
      seek(seconds, play) {
        command("seekTo", [seconds, true]);
        if (play) command("playVideo");
        clock.asked(seconds, play);
      },
      time: () => clock.now(),
      shown: () => clock.shown(),
      playing: () => clock.playing,
      togglePlay() {
        command(clock.playing ? "pauseVideo" : "playVideo");
        clock.set(clock.now());
        clock.playing = !clock.playing;
      },
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
          command("setPlaybackRate", [rate]);
        },
      },
      destroy: cleanup,
    }),
    // The player only reports to a page that has said it's listening.
    () => {
      const timer = setInterval(() => post({ event: "listening" }), 250);
      return () => clearInterval(timer);
    },
  );
}

const TWITCH = "https://player.twitch.tv";
// From Twitch's embed script (player.twitch.tv/js/embed/v1.js).
const TWITCH_PROXY = "twitch-embed-player-proxy";
const TWITCH_PAUSE = 2;
const TWITCH_PLAY = 3;
const TWITCH_SEEK = 4;

function twitch(
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
  const frame = sandboxedFrame(el, `${TWITCH}/?${params}`);
  const clock = new Clock(time, play);
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
        const moving = playback ? playback === "Playing" : undefined;
        if (typeof data.params?.currentTime === "number") clock.heardAt(data.params.currentTime, moving);
        else if (moving !== undefined) clock.heardMoving(moving);
        if (playback) clock.playing = ["Playing", "Buffering"].includes(playback);
        return true;
      }
      return data?.namespace === "twitch-embed" && (data.eventName === "ready" || data.eventName === "video.ready");
    },
    (cleanup) => ({
      seek(seconds, play) {
        command(TWITCH_SEEK, seconds);
        if (play) command(TWITCH_PLAY);
        clock.asked(seconds, play);
      },
      time: () => clock.now(),
      shown: () => clock.shown(),
      playing: () => clock.playing,
      togglePlay() {
        command(clock.playing ? TWITCH_PAUSE : TWITCH_PLAY);
        clock.set(clock.now());
        clock.playing = !clock.playing;
      },
      ownControls: true,
      destroy: cleanup,
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
    const unsupported = video.error?.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED;
    onerror?.(unsupported ? "This video's format can't be played here." : "This video couldn't be played.");
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
    volume: {
      get: () => video.volume,
      muted: () => video.muted,
      set(volume) {
        video.volume = volume;
        video.muted = volume === 0;
      },
      setMuted: (muted) => (video.muted = muted),
    },
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

/** Whether the app's own player (NativePlayer.svelte) plays here; asked once. */
let nativeHere: Promise<boolean> | null = null;

/** A file on this computer in the app's own player (NativePlayer.svelte),
 *  or where that can't draw yet, or the file is sound alone, streamed by
 *  the app's local media server into the webview's (LocalPlayer.svelte). */
async function local(
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
): Promise<EmbeddedPlayer> {
  nativeHere ??= api.playerAvailable().catch(() => false);
  if (!vod.audio_only && (await nativeHere)) {
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

/** A kept video, played like a local file; else the platform's player. */
function playerFor(vod: Vod): CreatePlayer | undefined {
  if (vod.media) return local;
  return Object.hasOwn(players, vod.platform) ? players[vod.platform] : undefined;
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

/** Whether `vod` plays from a file here, in the app's own player. */
export const playsHere = (vod: Vod) => playerFor(vod) === local;
