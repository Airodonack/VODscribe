// Controllers for the platforms' official embed APIs. They give a real
// seek(), so jumping between timestamps doesn't reload the player.

import Hls from "hls.js";
import { api, type Vod } from "./api";

export interface EmbeddedPlayer {
  seek(seconds: number, play: boolean): void;
  /** Current position in seconds. */
  time(): number;
  togglePlay(): void;
  destroy(): void;
}

const scripts = new Map<string, Promise<void>>();

function loadScript(src: string, ready?: (done: () => void) => void): Promise<void> {
  let p = scripts.get(src);
  if (!p) {
    p = new Promise<void>((resolve, reject) => {
      const el = document.createElement("script");
      el.src = src;
      el.async = true;
      el.onerror = () => {
        scripts.delete(src);
        reject(new Error(`Couldn't load ${src}`));
      };
      el.onload = () => (ready ? ready(resolve) : resolve());
      document.head.appendChild(el);
    });
    scripts.set(src, p);
  }
  return p;
}

/* eslint-disable @typescript-eslint/no-explicit-any */
declare global {
  interface Window {
    YT?: any;
    Twitch?: any;
    onYouTubeIframeAPIReady?: () => void;
  }
}

function loadYouTube(): Promise<void> {
  return loadScript("https://www.youtube.com/iframe_api", (done) => {
    if (window.YT?.Player) done();
    else window.onYouTubeIframeAPIReady = done;
  });
}

async function youtube(el: HTMLElement, vod: Vod, time: number, play: boolean): Promise<EmbeddedPlayer> {
  await loadYouTube();
  return new Promise((resolve) => {
    const player = new window.YT.Player(el, {
      host: "https://www.youtube-nocookie.com",
      videoId: vod.platform_id,
      width: "100%",
      height: "100%",
      playerVars: { start: Math.floor(time), autoplay: play ? 1 : 0, playsinline: 1, rel: 0 },
      events: {
        onReady: () =>
          resolve({
            seek(seconds, play) {
              player.seekTo(seconds, true);
              if (play) player.playVideo();
            },
            time: () => player.getCurrentTime(),
            togglePlay() {
              // State 1 is "playing".
              if (player.getPlayerState() === 1) player.pauseVideo();
              else player.playVideo();
            },
            destroy: () => player.destroy(),
          }),
      },
    });
  });
}

function twitchTime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 3600)}h${Math.floor((s % 3600) / 60)}m${s % 60}s`;
}

async function twitch(el: HTMLElement, vod: Vod, time: number, play: boolean): Promise<EmbeddedPlayer> {
  await loadScript("https://player.twitch.tv/js/embed/v1.js");
  return new Promise((resolve) => {
    const Player = window.Twitch.Player;
    const player = new Player(el, {
      video: vod.platform_id.replace(/^v/, ""),
      // Twitch refuses to play unless `parent` names the embedding page's host.
      parent: [location.hostname || "localhost"],
      time: twitchTime(time),
      autoplay: play,
      width: "100%",
      height: "100%",
    });
    player.addEventListener(Player.READY, () =>
      resolve({
        seek(seconds, play) {
          player.seek(seconds);
          if (play) player.play();
        },
        time: () => player.getCurrentTime(),
        togglePlay() {
          if (player.isPaused()) player.play();
          else player.pause();
        },
        destroy: () => {
          el.innerHTML = "";
        },
      }),
    );
  });
}
/* eslint-enable @typescript-eslint/no-explicit-any */

/** Kick has no VOD embed, so play its HLS stream in a plain <video>. */
async function kick(el: HTMLElement, vod: Vod, time: number, play: boolean): Promise<EmbeddedPlayer> {
  const variants = await api.kickStream(vod.url);
  // 720p is plenty for a preview pane and keeps bandwidth down.
  const pick =
    variants.filter((v) => v.height <= 720).sort((a, b) => b.height - a.height)[0] ?? variants[0];
  const video = document.createElement("video");
  video.controls = true;
  video.playsInline = true;
  video.style.cssText = "width: 100%; height: 100%; background: #000";
  el.appendChild(video);

  let hls: Hls | null = null;
  if (Hls.isSupported()) {
    hls = new Hls({ startPosition: time });
    hls.loadSource(pick.url);
    hls.attachMedia(video);
  } else {
    video.src = pick.url;
    video.currentTime = time;
  }
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
      hls?.destroy();
      video.remove();
    },
  };
}

/** Creates a player inside `el`, or returns null if the platform has no VOD embed. */
export function createPlayer(el: HTMLElement, vod: Vod, time: number, play: boolean): Promise<EmbeddedPlayer> | null {
  switch (vod.platform) {
    case "youtube":
      return youtube(el, vod, time, play);
    case "twitch":
      return twitch(el, vod, time, play);
    case "kick":
      return kick(el, vod, time, play);
    default:
      return null;
  }
}

export function canEmbed(vod: Vod): boolean {
  return vod.platform === "youtube" || vod.platform === "twitch" || vod.platform === "kick";
}
