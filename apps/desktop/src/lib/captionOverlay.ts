// Captions over the app's own player's picture. Nothing of the page draws
// over the native screen, so the page draws the caption into a canvas here
// and hands the pixels to the screen, which blends them over the picture
// (crates/player `Overlay`). Styled as the webview player's captions over
// its video (LocalPlayer.svelte): white on a dark box per line, centred
// near the bottom, larger in full screen.

import { api } from "./api";

/** The caption `text` drawn for a player box `width` by `height` CSS
 *  pixels, and where in the box it goes. */
export function drawCaption(
  text: string,
  width: number,
  height: number,
  fullscreen: boolean,
): { rect: { x: number; y: number; width: number; height: number }; canvas: HTMLCanvasElement } | null {
  const dpr = window.devicePixelRatio || 1;
  const vh = window.innerHeight / 100;
  const size = fullscreen ? 3.4 * vh : Math.min(Math.max(13, 2.4 * vh), 22);
  const bottom = fullscreen ? 4 * vh : 10;
  const pad = 8;
  const lineHeight = size * 1.35;
  const family = getComputedStyle(document.body).fontFamily;
  const font = `${size}px ${family}`;

  const measure = document.createElement("canvas").getContext("2d");
  if (!measure) return null;
  measure.font = font;
  // Words onto lines no wider than the caption's share of the box.
  const most = width * 0.84 - 2 * pad;
  const lines: string[] = [];
  for (const word of text.split(/\s+/).filter(Boolean)) {
    const last = lines.at(-1);
    if (last !== undefined && measure.measureText(`${last} ${word}`).width <= most)
      lines[lines.length - 1] = `${last} ${word}`;
    else lines.push(word);
  }
  if (!lines.length) return null;
  const widths = lines.map((l) => measure.measureText(l).width);
  const w = Math.ceil(Math.max(...widths) + 2 * pad);
  const h = Math.ceil(lines.length * lineHeight);
  // Whole device pixels, so the screen draws it pixel for pixel.
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(w * dpr);
  canvas.height = Math.ceil(h * dpr);
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.scale(dpr, dpr);
  ctx.font = font;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  lines.forEach((line, i) => {
    const lw = widths[i] + 2 * pad;
    const top = i * lineHeight;
    ctx.fillStyle = "rgb(0 0 0 / 0.72)";
    ctx.beginPath();
    ctx.roundRect((w - lw) / 2, top, lw, lineHeight, 4);
    ctx.fill();
    ctx.fillStyle = "#fff";
    ctx.fillText(line, w / 2, top + lineHeight / 2);
  });
  const rect = {
    x: (width - canvas.width / dpr) / 2,
    y: height - bottom - canvas.height / dpr,
    width: canvas.width / dpr,
    height: canvas.height / dpr,
  };
  return { rect, canvas };
}

/** Shows `text` as the caption over the screen's picture (none: no caption). */
export function showCaption(text: string | null, width: number, height: number, fullscreen: boolean) {
  const drawn = text ? drawCaption(text, width, height, fullscreen) : null;
  if (!drawn) {
    api.setScreenOverlay(null).catch(() => {});
    return;
  }
  const { canvas, rect } = drawn;
  const pixels = canvas.getContext("2d")!.getImageData(0, 0, canvas.width, canvas.height).data;
  api.setScreenOverlay({ rect, size: [canvas.width, canvas.height], pixels }).catch(() => {});
}
