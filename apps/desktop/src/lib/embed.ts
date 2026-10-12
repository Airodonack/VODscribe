// Controllers for the platforms' players. YouTube and Twitch run in sandboxed
// iframes on their own origins and are driven over postMessage, using the same
// messages their embed scripts send, so no third-party script runs in the app's
// window (where it could reach the app's commands). They still give a real
// seek(), so jumping between timestamps doesn't reload the player.

import { api, type Vod } from "./api";
import { twitchId, twitchTime } from "./platform";

export interface EmbeddedPlayer {
  seek(seconds: number, play: boolean): void;
  /** Current position in seconds. */
  time(): number;
  togglePlay(): void;
  destroy(): void;
}

/** The players' position as last reported, advanced by the clock while playing. */
class Clock {
  private at = performance.now();
  constructor(
    private seconds: number,
    public playing: boolean,
  ) {}
  set(seconds: number) {
    this.seconds = seconds;
    this.at = performance.now();
  }
  now() {
    return this.seconds + (this.playing ? (performance.now() - this.at) / 1000 : 0);
  }
}

function sandboxedFrame(el: HTMLElement, src: string): HTMLIFrameElement {
  const frame = document.createElement("iframe");
  // Scripts run on the player's own origin; it can't navigate the app or open windows.
  frame.setAttribute("sandbox", "allow-scripts allow-same-origin allow-presentation");
  frame.setAttribute("allow", "autoplay; fullscreen; encrypted-media; picture-in-picture");
  frame.setAttribute("allowfullscreen", "");
  frame.referrerPolicy = "strict-origin-when-cross-origin";
  frame.style.cssText = "width: 100%; height: 100%; border: 0";
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
  });
  const frame = sandboxedFrame(el, `${YOUTUBE}/embed/${encodeURIComponent(vod.platform_id)}?${params}`);
  const clock = new Clock(time, play);
  const post = (message: object) =>
    frame.contentWindow?.postMessage(JSON.stringify({ ...message, id: 1, channel: "widget" }), YOUTUBE);
  const command = (func: string, args: unknown[] = []) => post({ event: "command", func, args });

  return connect(
    frame,
    YOUTUBE,
    (raw) => {
      let data: { event?: string; info?: { currentTime?: number; playerState?: number } };
      try {
        data = typeof raw === "string" ? JSON.parse(raw) : (raw as typeof data);
      } catch {
        return false;
      }
      if (typeof data.info?.currentTime === "number") clock.set(data.info.currentTime);
      // State 1 is "playing", 3 is "buffering".
      if (typeof data.info?.playerState === "number") clock.playing = [1, 3].includes(data.info.playerState);
      return data.event === "onReady" || data.event === "initialDelivery";
    },
    (cleanup) => ({
      seek(seconds, play) {
        command("seekTo", [seconds, true]);
        if (play) command("playVideo");
        clock.set(seconds);
      },
      time: () => clock.now(),
      togglePlay() {
        command(clock.playing ? "pauseVideo" : "playVideo");
        clock.set(clock.now());
        clock.playing = !clock.playing;
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

function twitch(el: HTMLElement, vod: Vod, time: number, play: boolean): Promise<EmbeddedPlayer> {
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
        if (typeof data.params?.currentTime === "number") clock.set(data.params.currentTime);
        if (data.params?.playback) clock.playing = ["Playing", "Buffering"].includes(data.params.playback);
        return true;
      }
      return data?.namespace === "twitch-embed" && (data.eventName === "ready" || data.eventName === "video.ready");
    },
    (cleanup) => ({
      seek(seconds, play) {
        command(TWITCH_SEEK, seconds);
        if (play) command(TWITCH_PLAY);
        clock.set(seconds);
      },
      time: () => clock.now(),
      togglePlay() {
        command(clock.playing ? TWITCH_PAUSE : TWITCH_PLAY);
        clock.set(clock.now());
        clock.playing = !clock.playing;
      },
      destroy: cleanup,
    }),
  );
}

/** A plain <video> in `el`. `attach` points it at its source and returns any cleanup. */
function videoPlayer(
  el: HTMLElement,
  time: number,
  play: boolean,
  attach: (video: HTMLVideoElement) => (() => void) | void,
  onerror?: (message: string | null) => void,
): EmbeddedPlayer {
  const video = document.createElement("video");
  video.controls = true;
  video.playsInline = true;
  video.style.cssText = "width: 100%; height: 100%; background: #000";
  video.addEventListener("error", () => {
    const unsupported = video.error?.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED;
    onerror?.(unsupported ? "This video's format can't be played here." : "This video couldn't be played.");
  });
  // A later successful play (after a retry or seek) clears an earlier error.
  video.addEventListener("playing", () => onerror?.(null));
  el.appendChild(video);
  const detach = attach(video);
  if (play) video.play().catch(() => {});
  return {
    seek(seconds, play) {
      video.currentTime = seconds;
      if (play) video.play().catch(() => {});
    },
    time: () => video.currentTime,
    togglePlay() {
      if (video.paused) video.play().catch(() => {});
      else video.pause();
    },
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

/** A file on this computer, streamed by the app's local media server. */
async function local(
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
): Promise<EmbeddedPlayer> {
  const src = await api.mediaUrl(vod.id);
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

/** Creates a player inside `el`, or returns null if the platform has no VOD embed. */
export function createPlayer(
  el: HTMLElement,
  vod: Vod,
  time: number,
  play: boolean,
  onerror?: (message: string | null) => void,
): Promise<EmbeddedPlayer> | null {
  return players[vod.platform]?.(el, vod, time, play, onerror) ?? null;
}

export const canEmbed = (vod: Vod) => Object.hasOwn(players, vod.platform);
