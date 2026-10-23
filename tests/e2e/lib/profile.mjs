// Profiling a run (run.mjs --profile <dir>, Linux): perf follows the app
// and everything it starts (lib/profile.sh), one recording per start of
// the app, and after each scenario its recordings are read into
//
//   <dir>/<scenario>/<scenario>-<n>.data  perf's own files (perf report -i)
//   <dir>/<scenario>/folded.txt          every stack sampled, one per line:
//                                        process;thread;outermost;…;innermost count
//   <dir>/<scenario>/top.txt             where the time went, for reading (README)
//
// and a summary of it for perf.json. Each frame is put down to an owner by
// its library or symbol (`owner`): our code, the web engine, graphics, and
// so on, so the time in our own code reads apart from the engine's.

import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";

const here = path.dirname(fileURLToPath(import.meta.url));
const perf = () => process.env.PERF ?? "perf";

/**
 * Makes the run profiled: the driver starts lib/profile.sh, which starts
 * `app` under perf. Returns what to give the driver as the app. The
 * environment is set here, before the driver starts, since the driver's
 * processes (and so the app's) take theirs from it.
 */
export function profiled(dir, app, cg) {
  const found = spawnSync(perf(), ["--version"], { encoding: "utf8" });
  if (found.status !== 0) throw new Error(`--profile needs perf (${perf()}): install linux-perf (README)`);
  const raw = path.join(dir, "raw");
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(raw, { recursive: true });
  Object.assign(process.env, { PROFILE_OUT: raw, PROFILE_APP: path.resolve(app), PROFILE_CG: cg ?? "fp" });
  return path.join(here, "profile.sh");
}

/** The scenario the next recordings are for. */
export function recordingFor(dir, scenario) {
  writeFileSync(path.join(dir, "raw", ".scenario"), scenario);
}

/** Whether a process is still there. */
function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/**
 * After a scenario (its app closed): waits for perf to finish each of its
 * recordings, moves them to <dir>/<scenario>/, and writes folded.txt and
 * top.txt. `steps` are the scenario's timed steps (run.mjs), each with its
 * `from` and `to` on the monotonic clock, in seconds, as perf's samples are;
 * `page` is what the page measured (page.json), if anything. Returns the
 * summary for perf.json, or null with nothing recorded.
 */
export async function finish(dir, scenario, steps = [], page = null) {
  const raw = path.join(dir, "raw");
  const mine = readdirSync(raw).filter((f) => f.startsWith(`${scenario}-`));
  for (const f of mine.filter((f) => f.endsWith(".pid"))) {
    const pid = Number(readFileSync(path.join(raw, f), "utf8"));
    const end = Date.now() + 60_000;
    while (alive(pid) && Date.now() < end) await sleep(200);
    // Still going a minute after the app went: stopped as ^C would, which finishes its file.
    if (alive(pid)) process.kill(pid, "SIGINT");
    while (alive(pid)) await sleep(200);
    rmSync(path.join(raw, f));
  }
  if (!mine.some((f) => f.endsWith(".data"))) return null;
  const into = path.join(dir, scenario);
  mkdirSync(into, { recursive: true });
  for (const f of mine.filter((f) => !f.endsWith(".pid"))) renameSync(path.join(raw, f), path.join(into, f));
  const recordings = mine.filter((f) => f.endsWith(".data")).map((f) => path.join(into, f));
  const samples = recordings.flatMap(read);
  if (!samples.length) return null;
  writeFileSync(path.join(into, "folded.txt"), folded(samples));
  const summary = summarise(samples);
  writeFileSync(path.join(into, "top.txt"), top(scenario, samples, summary, steps, page));
  return summary;
}

/**
 * The samples in one recording: { time, process, thread, frames } with the
 * frames innermost first, each { sym, dso }.
 */
function read(file) {
  const script = execFileSync(perf(), ["script", "-i", file, "-F", "comm,pid,tid,time,ip,sym,dso"], {
    encoding: "utf8",
    maxBuffer: 2 ** 30,
    stdio: ["ignore", "pipe", "ignore"],
  });
  // perf can't read Rust's symbols (v0 mangling); binutils' c++filt can.
  const demangled = spawnSync("c++filt", [], { input: script, encoding: "utf8", maxBuffer: 2 ** 30 });
  const text = demangled.status === 0 ? demangled.stdout : script;
  const samples = [];
  for (const block of text.split(/\n\s*\n/)) {
    const [head, ...lines] = block.split("\n").filter((l) => l.trim());
    const m = head?.match(/^\s*(.+?)\s+(\d+)\/(\d+)\s+([\d.]+):\s*$/);
    if (!m) continue;
    const frames = lines
      .map((l) => l.match(/^\s+[0-9a-f]+\s+(.*?)\s+\((.*)\)\s*$/))
      .filter(Boolean)
      // Code made at run time (JavaScript's JIT, llvmpipe's shaders) has no file: perf names a map for it.
      .map(([, sym, dso]) => ({
        sym: tidy(sym),
        dso: /^perf-\d+\.map$/.test(path.basename(dso)) ? "JIT code" : path.basename(dso),
      }));
    samples.push({ comm: m[1], pid: Number(m[2]), tid: Number(m[3]), time: Number(m[4]), frames });
  }
  // A process by its main thread's name (a thread's own name is the thread's).
  const names = new Map(samples.filter((s) => s.pid === s.tid).map((s) => [s.pid, s.comm]));
  return samples.map(({ comm, pid, tid, time, frames }) => ({
    time,
    process: names.get(pid) ?? `pid ${pid}`,
    thread: comm,
    frames: frames.length ? frames : [{ sym: "[unknown]", dso: "[unknown]" }],
  }));
}

/**
 * A symbol as read, shorter: without Rust's crate hashes
 * (`vodscribe_player[698233fe7bd37a84]`) or a C++ function's parameters.
 */
function tidy(sym) {
  let name = sym.replace(/\[[0-9a-f]{5,16}\]/g, "");
  const tail = name.match(/\)( const)?$/);
  if (tail) {
    // Back to the parenthesis that opens the parameters.
    let depth = 0;
    for (let i = name.length - 1 - (tail[1]?.length ?? 0); i >= 0; i--) {
      if (name[i] === ")") depth++;
      else if (name[i] === "(" && --depth === 0) {
        name = name.slice(0, i);
        break;
      }
    }
  }
  return name;
}

/** A frame's name: its function, or its library where it has no name. */
function frameName({ sym, dso }) {
  return sym && sym !== "[unknown]" ? sym : `[${dso}]`;
}

/**
 * Whose a sample's innermost frame is: "ours" (our crates, vodscribe_*),
 * "crates" (the Rust libraries the app links in), "engine" (WebKit and
 * its JavaScript), "graphics" (wgpu, Mesa, software rendering), "media"
 * (ffmpeg), "transcription" (the worker, but for our own code in it), "toolkit" (GTK,
 * GLib, fonts), "system" (the kernel, libc).
 */
function owner(sample, frame = sample.frames[0]) {
  const { sym, dso } = frame;
  if (/^ffmpeg|^ffprobe/.test(sample.process)) return "media";
  if (/^llvmpipe|^lavapipe/.test(sample.thread)) return "graphics";
  // Ours by the function's path, demangled (`read`).
  if (/^<?vodscribe_/.test(sym)) return "ours";
  if (/^<?(wgpu|naga)/.test(sym)) return "graphics";
  // The worker's own libraries (the speech model's runtime, linked in) are transcription's.
  if (/^vodscribe-work/.test(sample.process)) return "transcription";
  if (/^vodscribe/.test(dso)) return "crates";
  if (/gallium|LLVM|vulkan|^libGL|^libEGL|gbm|drm|nvidia|pixman|cairo|mesa|lvp|dri/i.test(dso)) return "graphics";
  // In the web view's processes, the rest is the engine's work, whichever library does it.
  if (/webkit|javascriptcore/i.test(dso) || /^WebKit/.test(sample.process)) return "engine";
  if (/^libav|^libsw|^libpostproc/.test(dso)) return "media";
  if (/onnx|sherpa/i.test(dso)) return "transcription";
  if (/kernel|^\[vdso|^libc[.-]|^ld-linux|^libm[.-]|^libstdc|^libgcc|^libpthread/.test(dso)) return "system";
  if (/glib|gobject|gio|gtk|gdk|pango|fontconfig|freetype|harfbuzz|atk|soup|gst|icu|xml|expat|X11|xcb/i.test(dso))
    return "toolkit";
  return "other";
}

/** Every stack, folded (process;thread;outermost;…;innermost count), most sampled first. */
function folded(samples) {
  const counts = new Map();
  for (const s of samples) {
    const key = [s.process, s.thread, ...s.frames.map(frameName).reverse()].join(";");
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts]
    .sort((a, b) => b[1] - a[1])
    .map(([k, n]) => `${k} ${n}`)
    .join("\n")
    .concat("\n");
}

/** Counts by a key, most first, as [key, count]. */
function tally(items, key) {
  const counts = new Map();
  for (const item of items) {
    const k = key(item);
    if (k !== undefined) counts.set(k, (counts.get(k) ?? 0) + 1);
  }
  return [...counts].sort((a, b) => b[1] - a[1]);
}

const pct = (n, total) => `${((100 * n) / Math.max(total, 1)).toFixed(1)}%`;

/** For perf.json: samples, the share by owner, and our own code's top functions. */
function summarise(samples) {
  const total = samples.length;
  const owners = Object.fromEntries(
    tally(samples, (s) => owner(s)).map(([k, n]) => [k, Number(((100 * n) / total).toFixed(1))]),
  );
  // Ours anywhere on the stack: work our code asked for, inside other code included.
  const under = samples.filter((s) => s.frames.some((f) => owner(s, f) === "ours")).length;
  const ours = tally(
    samples.filter((s) => owner(s) === "ours"),
    (s) => frameName(s.frames[0]),
  )
    .slice(0, 5)
    .map(([fn, n]) => ({ fn, percent: Number(((100 * n) / total).toFixed(2)) }));
  return { samples: total, owners, underOurs: Number(((100 * under) / total).toFixed(1)), ours };
}

/** top.txt: where a scenario's time went, by process, owner, function and step. */
function top(scenario, samples, summary, steps, page) {
  const total = samples.length;
  const lines = [`${scenario}: ${total} samples (on-CPU, all processes)`, ""];
  const section = (title) => lines.push("", `== ${title}`);
  const rows = (pairs, of = total, limit = 20) =>
    pairs.slice(0, limit).forEach(([k, n]) => lines.push(`${pct(n, of).padStart(7)}  ${k}`));

  section("By process and thread");
  rows(tally(samples, (s) => `${s.process} / ${s.thread}`));

  section("By owner (innermost frame)");
  rows(tally(samples, (s) => owner(s)));
  lines.push(`${`${summary.underOurs}%`.padStart(7)}  with our code anywhere on the stack`);

  section("Our code: functions by own time, with who called them");
  const ours = samples.filter((s) => owner(s) === "ours");
  for (const [fn, n] of tally(ours, (s) => frameName(s.frames[0])).slice(0, 30)) {
    lines.push(`${pct(n, total).padStart(7)}  ${fn}`);
    const callers = tally(
      ours.filter((s) => frameName(s.frames[0]) === fn),
      (s) => s.frames.slice(1, 3).map(frameName).join(" < ") || undefined,
    );
    for (const [c, m] of callers.slice(0, 2)) lines.push(`           < ${c} (${m})`);
  }
  if (!ours.length) lines.push("  (none sampled)");

  section("Functions by own time, everyone's");
  rows(
    tally(samples, (s) => `${frameName(s.frames[0])}  [${owner(s)}, ${s.process}]`),
    total,
    30,
  );

  for (const step of steps.filter((t) => t.from !== undefined)) {
    const inside = samples.filter((s) => s.time >= step.from && s.time <= step.to);
    section(`Step: ${step.name} (${step.ms} ms, ${inside.length} samples)`);
    rows(
      tally(inside, (s) => owner(s)),
      inside.length,
    );
    lines.push("  top functions:");
    rows(
      tally(inside, (s) => `${frameName(s.frames[0])}  [${owner(s)}, ${s.thread}]`),
      inside.length,
      10,
    );
  }

  if (page) {
    section("The page");
    const slow = page.slowEvents ?? [];
    const gaps = page.frameGaps ?? [];
    lines.push(`  ${slow.length} input event(s) handled in 16 ms or more; ${gaps.length} frame gap(s) over 50 ms`);
    for (const e of [...slow].sort((a, b) => b.duration - a.duration).slice(0, 10))
      lines.push(`  event ${e.name} on ${e.target ?? "?"}: ${e.duration} ms at ${e.at} ms`);
    for (const g of [...gaps].sort((a, b) => b.ms - a.ms).slice(0, 10))
      lines.push(`  frame gap: ${g.ms} ms at ${g.at} ms`);
  }
  return lines.join("\n").concat("\n");
}
