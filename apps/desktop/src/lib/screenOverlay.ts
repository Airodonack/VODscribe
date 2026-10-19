// What's drawn over the app's own player's picture. Nothing of the page
// draws over the native screen, so the page draws these into a canvas here
// and hands the pixels to the screen, which blends them over the picture
// (crates/player `Overlay`): the caption (styled as the webview player's
// captions over its video, LocalPlayer.svelte), and in the edit view its
// hint and the frame a dragged edge is on (EditView.svelte's styles).

import { api } from "./api";
import { wrapWith } from "./textWrap";

/** A canvas and the box in the player's box (CSS pixels) it's shown in. */
export interface Drawn {
  rect: { x: number; y: number; width: number; height: number };
  canvas: HTMLCanvasElement;
}

/** A canvas `width` by `height` CSS pixels at the screen's pixel density, drawn on by `draw`. */
function canvasOf(
  width: number,
  height: number,
  draw: (ctx: CanvasRenderingContext2D) => void,
): HTMLCanvasElement | null {
  const dpr = window.devicePixelRatio || 1;
  const canvas = document.createElement("canvas");
  // Whole device pixels, so the screen draws it pixel for pixel.
  canvas.width = Math.max(1, Math.ceil(width * dpr));
  canvas.height = Math.max(1, Math.ceil(height * dpr));
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.scale(dpr, dpr);
  draw(ctx);
  return canvas;
}

/** A path round the box at `x`, `y`, `w` by `h`, its corners of radius `r`
 *  (as `roundRect`, which macOS 12's WebKit lacks). */
function rounded(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  r = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

/** The page's font at `size` pixels. */
function fontOf(size: number) {
  return `${size}px ${getComputedStyle(document.body).fontFamily}`;
}

/** `text`'s words onto lines no wider than `most`, in `font` (textWrap.ts). */
function wrap(text: string, font: string, most: number): { lines: string[]; widths: number[] } {
  const measure = document.createElement("canvas").getContext("2d");
  if (!measure) return { lines: [], widths: [] };
  measure.font = font;
  const width = (s: string) => measure.measureText(s).width;
  const lines = wrapWith(text, most, width);
  return { lines, widths: lines.map(width) };
}

/** `canvas` (from `canvasOf`) placed at `x`, `y` CSS pixels in the box, on
 *  whole device pixels as it was drawn: at a fractional scale (125%) a box
 *  between them would be resampled by the screen, and blur. */
function placed(canvas: HTMLCanvasElement, x: number, y: number): Drawn {
  const dpr = window.devicePixelRatio || 1;
  const snap = (v: number) => Math.round(v * dpr) / dpr;
  return { rect: { x: snap(x), y: snap(y), width: canvas.width / dpr, height: canvas.height / dpr }, canvas };
}

/** The caption `text` for a player box `width` by `height`: white on a
 *  dark box per line, centred near the bottom, larger in full screen. */
export function caption(text: string, width: number, height: number, fullscreen: boolean): Drawn | null {
  const vh = window.innerHeight / 100;
  const size = fullscreen ? 3.4 * vh : Math.min(Math.max(13, 2.4 * vh), 22);
  const bottom = fullscreen ? 4 * vh : 10;
  const pad = 8;
  const lineHeight = size * 1.35;
  const font = fontOf(size);
  const { lines, widths } = wrap(text, font, width * 0.84 - 2 * pad);
  if (!lines.length) return null;
  const w = Math.ceil(Math.max(...widths) + 2 * pad);
  const h = Math.ceil(lines.length * lineHeight);
  const canvas = canvasOf(w, h, (ctx) => {
    ctx.font = font;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    lines.forEach((line, i) => {
      const lw = widths[i] + 2 * pad;
      const top = i * lineHeight;
      ctx.fillStyle = "rgb(0 0 0 / 0.72)";
      rounded(ctx, (w - lw) / 2, top, lw, lineHeight, 4);
      ctx.fill();
      ctx.fillStyle = "#fff";
      ctx.fillText(line, w / 2, top + lineHeight / 2);
    });
  });
  if (!canvas) return null;
  return placed(canvas, (width - w) / 2, height - bottom - h);
}

/** A note `text` across the bottom of a player box `width` by `height`
 *  (the edit view's hint): white on a dark box, centred. */
export function hint(text: string, width: number, height: number): Drawn | null {
  const [inset, padX, padY, size] = [24, 14, 10, 13];
  const lineHeight = size * 1.5;
  const font = fontOf(size);
  const w = Math.max(1, width - 2 * inset);
  const { lines } = wrap(text, font, w - 2 * padX);
  if (!lines.length) return null;
  const h = Math.ceil(lines.length * lineHeight + 2 * padY);
  const canvas = canvasOf(w, h, (ctx) => {
    ctx.fillStyle = "rgb(0 0 0 / 0.7)";
    rounded(ctx, 0, 0, w, h, 6);
    ctx.fill();
    ctx.font = font;
    ctx.fillStyle = "#fff";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    lines.forEach((line, i) => ctx.fillText(line, w / 2, padY + (i + 0.5) * lineHeight));
  });
  if (!canvas) return null;
  return placed(canvas, inset, height - inset - h);
}

/** The whole player box `width` by `height`: `picture` (if any) fitted on
 *  black, and `label` on a dark pill at the top (a dragged edge's frame). */
export function framed(picture: HTMLImageElement | null, label: string, width: number, height: number): Drawn | null {
  const canvas = canvasOf(width, height, (ctx) => {
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, width, height);
    if (picture?.naturalWidth) {
      const fit = Math.min(width / picture.naturalWidth, height / picture.naturalHeight);
      const [pw, ph] = [picture.naturalWidth * fit, picture.naturalHeight * fit];
      ctx.drawImage(picture, (width - pw) / 2, (height - ph) / 2, pw, ph);
    }
    ctx.font = fontOf(12);
    const lw = Math.min(ctx.measureText(label).width + 20, width - 16);
    ctx.fillStyle = "rgb(0 0 0 / 0.7)";
    rounded(ctx, (width - lw) / 2, 12, lw, 24, 6);
    ctx.fill();
    ctx.fillStyle = "#fff";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(label, width / 2, 24, lw - 20);
  });
  if (!canvas) return null;
  return placed(canvas, 0, 0);
}

/** Shows `drawn` over the screen's picture (none: nothing). */
export function show(drawn: Drawn | null) {
  if (!drawn) {
    api.setScreenOverlay(null).catch(() => {});
    return;
  }
  const { canvas, rect } = drawn;
  const pixels = canvas.getContext("2d")!.getImageData(0, 0, canvas.width, canvas.height).data;
  api.setScreenOverlay({ rect, size: [canvas.width, canvas.height], pixels }).catch(() => {});
}
