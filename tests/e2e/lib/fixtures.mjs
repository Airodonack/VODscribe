// Test files: the recorded ones in fixtures/, and videos made at test time
// with the ffmpeg setup downloaded (or one on PATH), so the repository holds
// no video files.

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { dataDirs } from "./data.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
export const LANDED = path.join(here, "..", "fixtures", "eagle-has-landed.mp3");

function ffmpeg() {
  const exe = process.platform === "win32" ? "ffmpeg.exe" : "ffmpeg";
  const downloaded = path.join(dataDirs().at(-1), "bin", exe);
  return existsSync(downloaded) ? downloaded : exe;
}

/** Runs ffmpeg to make `file` (in `dir`) with `args` between the inputs and it. */
function make(dir, file, args) {
  const out = path.join(dir, file);
  if (!existsSync(out)) execFileSync(ffmpeg(), ["-loglevel", "error", "-y", ...args, out], { stdio: "inherit" });
  return out;
}

/** The videos, made in `dir` once; returns their paths by kind. */
export function videos(dir) {
  mkdirSync(dir, { recursive: true });
  const pattern = (size) => ["-f", "lavfi", "-i", `testsrc2=size=${size}:rate=30:duration=5`];
  const h264 = ["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"];
  const speech = ["-i", LANDED, "-c:a", "aac", "-shortest"];
  const video = make(dir, "landed.mp4", [...pattern("640x360"), ...speech, ...h264]);
  // A name with letters from other scripts, spaces and a dash, as people's files have.
  const named = path.join(dir, "Ünïcødé name — テスト 1.mp4");
  if (!existsSync(named)) copyFileSync(video, named);
  return {
    video,
    named,
    // Wider than any screen, and than the player's largest frame (MAX_SIDE).
    wide: make(dir, "wide.mp4", [...pattern("5000x400"), ...speech, ...h264]),
    // Sound, but nothing said.
    silent: make(dir, "silent.mp4", [
      ...pattern("640x360"),
      ...["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo", "-t", "5", "-c:a", "aac"],
      ...h264,
    ]),
    // No sound at all.
    mute: make(dir, "mute.mp4", [...pattern("640x360"), "-an", ...h264]),
  };
}

/** Ten minutes of speech (the landing, over and over), made in `dir` once: long enough to quit in the middle of. */
export function longSpeech(dir) {
  mkdirSync(dir, { recursive: true });
  return make(dir, "landed-10-minutes.m4a", ["-stream_loop", "119", "-i", LANDED, "-c:a", "aac", "-b:a", "64k"]);
}

/** The colour of each second of colours.mp4, in order. */
export const COLOURS = [
  ["red", [255, 0, 0]],
  ["lime", [0, 255, 0]],
  ["blue", [0, 0, 255]],
  ["yellow", [255, 255, 0]],
  ["magenta", [255, 0, 255]],
  ["cyan", [0, 255, 255]],
];

/**
 * A video whose picture says where it is: each second one flat colour
 * (COLOURS), so a screenshot tells which frame shows without reading it.
 * No sound. Made in `dir` once.
 */
export function colours(dir) {
  mkdirSync(dir, { recursive: true });
  const inputs = COLOURS.flatMap(([name]) => ["-f", "lavfi", "-i", `color=c=${name}:s=640x360:r=30:d=1`]);
  const joined = `${COLOURS.map((_, i) => `[${i}:v]`).join("")}concat=n=${COLOURS.length}:v=1:a=0[v]`;
  return make(dir, "colours.mp4", [
    ...inputs,
    "-filter_complex",
    joined,
    "-map",
    "[v]",
    "-c:v",
    "libx264",
    "-preset",
    "ultrafast",
    "-pix_fmt",
    "yuv420p",
    "-g",
    "30",
  ]);
}
