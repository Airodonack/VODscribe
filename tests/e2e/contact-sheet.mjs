// A contact sheet of the end-to-end tests' checkpoints (ctx.checkpoint), for
// looking at: one row per checkpoint, one column per run (a runner's
// out/<name>/, or out/ itself), each picture where it was taken. It's the
// layer the tests can't judge: whether it looks right.
//
//   node tests/e2e/contact-sheet.mjs <out dir>... [--out sheet.html] [--title "..."]
//
// Pictures are scaled down and embedded, so the page stands alone. verify.mjs
// makes its verdict sheet with the same pieces (jpeg, esc, sheet).

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";

/** The checkpoints a run took (its out dir's checkpoints/index.json), none if it took none. */
export function checkpointsOf(dir) {
  const index = path.join(dir, "checkpoints", "index.json");
  return existsSync(index) ? JSON.parse(readFileSync(index, "utf8")) : [];
}

/** A picture scaled to at most 960 wide, as a base64 JPEG. */
export const jpeg = (file) =>
  execFileSync(
    "ffmpeg",
    ["-loglevel", "error", "-i", file, "-vf", "scale='min(960,iw)':-2", "-q:v", "5", "-f", "mjpeg", "-"],
    {
      maxBuffer: 1 << 26,
    },
  ).toString("base64");

export const esc = (s) =>
  String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

/** The page around a sheet's table: its title, the shared look, and `style` for what a sheet adds. */
export const sheet = (title, table, style = "") => `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)}</title>
<style>
  :root { --bg: #fafaf8; --fg: #1d1d1b; --muted: #6b6b66; --line: #e2e2dc; }
  @media (prefers-color-scheme: dark) { :root { --bg: #161615; --fg: #ecece8; --muted: #9a9a93; --line: #2c2c2a; } }
  body { margin: 0; padding: 16px; background: var(--bg); color: var(--fg); font: 14px/1.4 system-ui, sans-serif; }
  h1 { font-size: 18px; margin: 0 0 12px; }
  .wrap { overflow-x: auto; }
  table { border-collapse: collapse; }
  th, td { border-top: 1px solid var(--line); padding: 8px; vertical-align: top; text-align: left; }
  th.row { position: sticky; left: 0; background: var(--bg); font-weight: 600; white-space: nowrap; }
  td img { display: block; width: 420px; max-width: 70vw; height: auto; border: 1px solid var(--line); }
  .from, .none { color: var(--muted); font-size: 12px; }${style}
</style></head>
<body><h1>${esc(title)}</h1><div class="wrap"><table>
${table}
</table></div></body></html>
`;

if (import.meta.main) {
  const { values: opts, positionals: dirs } = parseArgs({
    allowPositionals: true,
    options: { out: { type: "string" }, title: { type: "string" } },
  });
  if (!dirs.length) {
    console.error('usage: node tests/e2e/contact-sheet.mjs <out dir>... [--out sheet.html] [--title "..."]');
    process.exit(2);
  }

  const runs = dirs.map((dir) => ({ name: path.basename(path.resolve(dir)), dir, list: checkpointsOf(dir) }));
  // Rows in the order the checkpoints were first taken, across the runs.
  const rows = [];
  for (const run of runs)
    for (const c of run.list) if (!rows.includes(`${c.scenario}/${c.label}`)) rows.push(`${c.scenario}/${c.label}`);

  const cells = (row) =>
    runs
      .map((run) => {
        const c = run.list.find((c) => `${c.scenario}/${c.label}` === row);
        if (!c) return `<td class="none">not taken</td>`;
        const img = jpeg(path.join(run.dir, "checkpoints", c.file));
        return `<td><img loading="lazy" src="data:image/jpeg;base64,${img}" alt="${esc(row)} on ${esc(run.name)}"><span class="from">${esc(c.from)} · ${esc(c.platform)}</span></td>`;
      })
      .join("");

  const table = `<tr><th></th>${runs.map((r) => `<th>${esc(r.name)}</th>`).join("")}</tr>
${rows.map((row) => `<tr><th class="row">${esc(row)}</th>${cells(row)}</tr>`).join("\n")}`;
  const file = opts.out ?? path.join(dirs[0], "contact-sheet.html");
  writeFileSync(file, sheet(opts.title ?? "End-to-end checkpoints", table));
  console.log(`${rows.length} checkpoints from ${runs.length} runs: ${file}`);
}
