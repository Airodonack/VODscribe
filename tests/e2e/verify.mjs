// The top layer of the tests: a model looks at the checkpoints (ctx.checkpoint)
// and says whether each shows what a person should see there, the judgement
// no check in a scenario can make (a picture cut off, text over text, a
// colour off-theme). A scenario says what each checkpoint should show beside
// its `run`: `expects: { "<checkpoint label>": "<one or two sentences>" }`.
//
//   node tests/e2e/verify.mjs <out dir>... [--model <id>] [--out verify.html]
//     [--only a,b] [--concurrency N] [--dry-run]
//     [--manifest <file> | --verdicts <file>]
//
// Each checkpoint with an expectation is one request to OpenRouter's chat
// completions (OPENROUTER_API_KEY), with its picture, the expectation and a
// fixed rubric; the answer is pass, fail or unsure, with a reason and
// anything that looks broken even if not asked. Checkpoints without an
// expectation are listed, not judged. The model: --model or
// E2E_VERIFY_MODEL, an exact model id, or a prefix ending in "*" for the
// cheapest model with that prefix that takes pictures, looked up at run time.
// verify.json (each verdict) goes beside the verdict sheet (--out, default
// the first out dir's verify.html), fails first. Exit 1 on a fail; unsure
// doesn't fail. --dry-run prints the pairs and the prompt, calling nothing.
//
// Without a key, an agent (a person, or a model with a file-reading tool)
// judges the pictures instead:
//   --manifest <file>  writes the rubric, the pairs (absolute picture path,
//                      scenario, label, out dir, expectation) and the shape
//                      of one answer line, and exits 0, calling nothing.
//   --verdicts <file>  reads the agent's answers, one JSON object per line
//                      ({file, verdict, reason, concerns}; `file` is a
//                      picture's basename or absolute path), and does all
//                      that the OpenRouter path does with its answers: the
//                      sheet, verify.json, the summary, exit 1 on a fail. A
//                      picture with no answer is unsure, "no verdict".
// Both take --only.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { setTimeout as sleep } from "node:timers/promises";

import { checkpointsOf, esc, jpeg, sheet } from "./contact-sheet.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const API = "https://openrouter.ai/api/v1";
const VERDICTS = ["pass", "fail", "unsure"];

/** What the model is told for every picture; the expectation and the picture follow it. */
export const RUBRIC = `You check screenshots for the end-to-end tests of a desktop app that plays, transcribes and clips videos. Each time you get one screenshot of the app's window and an expectation: what a person should be able to confirm from the picture.

Judge only what is visible in the picture. Don't assume anything it doesn't show, and don't fail it for what the expectation doesn't ask for.
- "pass": the picture plainly shows what is expected.
- "fail": it plainly doesn't, or shows something that contradicts it.
- "unsure": the picture can't tell (too small, hidden, ambiguous).

Separately, list as concerns anything in the picture that looks broken, cropped, overlapping, unreadable or off-theme, even if the expectation doesn't mention it; an empty list if nothing does.

Answer with a JSON object and nothing else:
{"verdict": "pass" | "fail" | "unsure", "reason": "<one sentence>", "concerns": ["<a short phrase each>"]}`;

/** The messages for one checkpoint: the rubric, then its expectation and its picture (a base64 JPEG). */
export const messages = (expectation, picture) => [
  { role: "system", content: RUBRIC },
  {
    role: "user",
    content: [
      { type: "text", text: `Expectation: ${expectation}` },
      { type: "image_url", image_url: { url: `data:image/jpeg;base64,${picture}` } },
    ],
  },
];

/**
 * The model's answer as { verdict, reason, concerns }: the JSON object in
 * it, fenced or not. Throws when there's none or it isn't of that shape.
 */
export function parseVerdict(text) {
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start < 0 || end < start) throw new Error("no JSON object in the answer");
  const answer = JSON.parse(text.slice(start, end + 1));
  const verdict = String(answer.verdict ?? "").toLowerCase();
  if (!VERDICTS.includes(verdict)) throw new Error(`the verdict is ${JSON.stringify(answer.verdict)}`);
  if (typeof answer.reason !== "string" || !answer.reason.trim()) throw new Error("no reason");
  const concerns = answer.concerns ?? [];
  if (!Array.isArray(concerns)) throw new Error("concerns isn't a list");
  return { verdict, reason: answer.reason.trim(), concerns: concerns.map(String).filter((c) => c.trim()) };
}

/**
 * Each run's checkpoints paired with their scenario's expectation
 * (`expects`: scenario name to { label: expectation }). Returns the pairs
 * to judge, the checkpoints without one, and the expectations a scenario
 * that ran didn't take a checkpoint for (a label renamed, a step skipped).
 * `only`, if given, keeps those scenarios.
 */
export function pair(runs, expects, only = null) {
  const judged = [];
  const unpaired = [];
  const missing = [];
  for (const run of runs) {
    const list = run.list.filter((c) => !only || only.includes(c.scenario));
    for (const c of list) {
      const entry = {
        run: run.name,
        scenario: c.scenario,
        label: c.label,
        file: path.join(run.dir, "checkpoints", c.file),
      };
      const expectation = expects[c.scenario]?.[c.label];
      if (expectation) judged.push({ ...entry, expectation });
      else unpaired.push(entry);
    }
    for (const scenario of new Set(list.map((c) => c.scenario)))
      for (const label of Object.keys(expects[scenario] ?? {}))
        if (!list.some((c) => c.scenario === scenario && c.label === label))
          missing.push({ run: run.name, scenario, label });
  }
  return { judged, unpaired, missing };
}

/**
 * The model to use: `wanted` itself, or for a prefix ending in "*" the
 * cheapest (prompt price) of `models` (OpenRouter's list) with that prefix
 * that takes pictures, variants left out. Returns { id, prompt, completion } (USD per token).
 */
export function pickModel(wanted, models) {
  const price = (m) => ({ id: m.id, prompt: Number(m.pricing?.prompt), completion: Number(m.pricing?.completion) });
  if (!wanted.endsWith("*")) {
    const m = models.find((m) => m.id === wanted);
    return m ? price(m) : { id: wanted, prompt: NaN, completion: NaN };
  }
  const prefix = wanted.slice(0, -1);
  // Not a variant (":batch", ":free"): those answer differently, or not at once.
  const fits = models
    .filter((m) => m.id.startsWith(prefix) && !m.id.includes(":"))
    .filter((m) => (m.architecture?.input_modalities ?? ["image"]).includes("image"))
    .map(price)
    .filter((m) => Number.isFinite(m.prompt))
    .sort((a, b) => a.prompt - b.prompt || a.id.localeCompare(b.id));
  if (!fits.length) throw new Error(`no model on OpenRouter starts with ${prefix} and takes pictures`);
  return fits[0];
}

/** The answer format an agent is asked for, one JSON object per line of the verdicts file. */
export const ANSWER_FORMAT = `One line of JSON per picture, no other text: {"file": "<the picture's file name or absolute path>", "verdict": "pass" | "fail" | "unsure", "reason": "<one sentence>", "concerns": ["<a short phrase each>"]}`;

/** The manifest an agent judges from: the rubric, the answer format and the pairs, the picture paths absolute. */
export function manifestOf(judged) {
  return {
    rubric: RUBRIC,
    answer: ANSWER_FORMAT,
    pairs: judged.map((j) => ({
      file: path.resolve(j.file),
      scenario: j.scenario,
      label: j.label,
      outDir: path.dirname(path.dirname(path.resolve(j.file))),
      expectation: j.expectation,
    })),
  };
}

/**
 * An agent's verdicts (JSON Lines text) joined to the pairs: each pair's
 * verdict as the model path would have made it. `file` matches a pair by
 * absolute path or by basename (the first pair with it not yet answered).
 * A pair with no answer is unsure, "no verdict". Returns { verdicts,
 * stray }: stray are the answers that matched no pair. Throws on a line
 * that isn't a verdict, naming the line.
 */
export function joinVerdicts(text, judged) {
  const answers = [];
  text.split("\n").forEach((line, i) => {
    if (!line.trim()) return;
    try {
      const a = JSON.parse(line);
      if (typeof a?.file !== "string" || !a.file) throw new Error("no file");
      answers.push({ file: a.file, ...parseVerdict(JSON.stringify(a)) });
    } catch (e) {
      throw new Error(`verdicts line ${i + 1}: ${e.message}`);
    }
  });
  const taken = new Map();
  const stray = [];
  for (const a of answers) {
    const abs = path.isAbsolute(a.file);
    const index = judged.findIndex(
      (j, i) =>
        !taken.has(i) && (abs ? path.resolve(j.file) === path.resolve(a.file) : path.basename(j.file) === a.file),
    );
    if (index < 0) stray.push(a.file);
    else taken.set(index, a);
  }
  const verdicts = judged.map((j, i) => {
    const base = {
      scenario: j.scenario,
      label: j.label,
      run: j.run,
      expectation: j.expectation,
      file: j.file,
      model: "agent",
      tokens: null,
    };
    const a = taken.get(i);
    return a
      ? { ...base, verdict: a.verdict, reason: a.reason, concerns: a.concerns }
      : { ...base, verdict: "unsure", reason: "no verdict", concerns: [] };
  });
  return { verdicts, stray };
}

/** The line a run ends with. */
export function summary(verdicts, unpaired) {
  const count = (v) => verdicts.filter((r) => r.verdict === v).length;
  const tokens = verdicts.reduce((sum, r) => sum + (r.tokens?.prompt ?? 0) + (r.tokens?.completion ?? 0), 0);
  return `verify: ${count("pass")} pass, ${count("fail")} fail, ${count("unsure")} unsure, ${unpaired} without expectation; ~${tokens} tokens`;
}

/** A refusal that no retry mends (the key, the credit, the request itself). */
class Refused extends Error {}

/** POST to OpenRouter with the key, the answer's JSON; 429 and 5xx retried with backoff, 60 s each try. */
async function post(route, body, key, tries = 4) {
  for (let attempt = 1; ; attempt++) {
    let res;
    try {
      res = await fetch(`${API}${route}`, {
        method: "POST",
        headers: { authorization: `Bearer ${key}`, "content-type": "application/json" },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(60_000),
      });
    } catch (e) {
      // A timeout or a dropped connection: worth another try.
      if (attempt >= tries)
        throw new Error(`no answer: ${e.name === "TimeoutError" ? "timed out after 60 s" : e.message}`);
      await sleep(2 ** attempt * 1000);
      continue;
    }
    if (res.ok) return res.json();
    // Only the status and the API's own message: never the request.
    const said = await res
      .json()
      .then((j) => j?.error?.message ?? "")
      .catch(() => "");
    const why = `OpenRouter answered ${res.status}${said ? `: ${String(said).slice(0, 200)}` : ""}`;
    if (res.status !== 429 && res.status < 500) throw new Refused(why);
    if (attempt >= tries) throw new Error(why);
    const after = Number(res.headers.get("retry-after"));
    await sleep(Number.isFinite(after) && after > 0 ? Math.min(after, 60) * 1000 : 2 ** attempt * 1000);
  }
}

/** One checkpoint judged: its verdict, or unsure with why there's none. */
async function judge(item, model, key) {
  const picture = jpeg(item.file);
  const base = {
    scenario: item.scenario,
    label: item.label,
    run: item.run,
    expectation: item.expectation,
    model: model.id,
  };
  let answer;
  try {
    answer = await post(
      "/chat/completions",
      {
        model: model.id,
        messages: messages(item.expectation, picture),
        temperature: 0,
        max_tokens: 400,
        usage: { include: true },
      },
      key,
    );
  } catch (e) {
    if (e instanceof Refused) throw e;
    return { ...base, verdict: "unsure", reason: e.message, concerns: [], tokens: null, picture };
  }
  const tokens = { prompt: answer.usage?.prompt_tokens ?? 0, completion: answer.usage?.completion_tokens ?? 0 };
  const cost = answer.usage?.cost;
  const text = answer.choices?.[0]?.message?.content ?? "";
  try {
    return { ...base, ...parseVerdict(text), tokens, cost, picture };
  } catch (e) {
    return {
      ...base,
      verdict: "unsure",
      reason: `the answer wasn't the JSON asked for (${e.message})`,
      concerns: [],
      tokens,
      cost,
      picture,
    };
  }
}

/** Runs `work` over `items`, at most `n` at once, the results in the items' order. */
async function pool(items, n, work) {
  const results = new Array(items.length);
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const i = next++;
      results[i] = await work(items[i], i);
    }
  };
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, worker));
  return results;
}

/** Each named scenario's expectations, read from its module (none for one that isn't there or can't load). */
async function expectationsOf(names) {
  const expects = {};
  for (const name of names) {
    const file = path.join(here, "scenarios", `${name}.mjs`);
    if (!existsSync(file)) continue;
    try {
      const { default: scenario } = await import(pathToFileURL(file));
      if (scenario?.expects) expects[name] = scenario.expects;
    } catch (e) {
      console.error(`  ${name}: can't read its expectations (${e.message.split("\n")[0]})`);
    }
  }
  return expects;
}

const ORDER = { fail: 0, unsure: 1, pass: 2 };

function verdictSheet(verdicts, unpaired, title) {
  const row = (
    r,
  ) => `<tr class="${r.verdict ?? "none"}"><th class="row">${esc(`${r.scenario}/${r.label}`)}<span class="from">${esc(r.run)}</span></th>
<td><img loading="lazy" src="data:image/jpeg;base64,${r.picture ?? jpeg(r.file)}" alt="${esc(`${r.scenario}/${r.label}`)}"></td>
<td class="say">${
    r.verdict
      ? `<p class="verdict">${esc(r.verdict)}</p><p>${esc(r.reason)}</p>${
          r.concerns.length ? `<ul>${r.concerns.map((c) => `<li>${esc(c)}</li>`).join("")}</ul>` : ""
        }<p class="from">Expected: ${esc(r.expectation)}</p>`
      : `<p class="none">no expectation</p>`
  }</td></tr>`;
  const sorted = [...verdicts].sort((a, b) => ORDER[a.verdict] - ORDER[b.verdict]);
  const style = `
  .say { max-width: 420px; }
  .say p { margin: 0 0 6px; }
  .say ul { margin: 0 0 6px; padding-left: 18px; }
  .row .from { display: block; font-weight: 400; }
  .verdict { font-weight: 700; text-transform: uppercase; letter-spacing: 0.04em; }
  .fail .verdict { color: #c62828; }
  .unsure .verdict { color: #b26a00; }
  .pass .verdict { color: #2e7d32; }
  @media (prefers-color-scheme: dark) {
    .fail .verdict { color: #ef6b6b; } .unsure .verdict { color: #f0a640; } .pass .verdict { color: #6cc070; }
  }`;
  return sheet(title, [...sorted, ...unpaired].map(row).join("\n"), style);
}

/** What both paths do with the verdicts: verify.json, the sheet, the summary line, exit 1 on a fail. */
function finish(verdicts, unpaired, dirs, opts) {
  const sheetFile = opts.out ?? path.join(dirs[0], "verify.html");
  const jsonFile = path.join(path.dirname(sheetFile), "verify.json");
  mkdirSync(path.dirname(sheetFile), { recursive: true });
  writeFileSync(
    jsonFile,
    JSON.stringify(
      verdicts.map(({ picture, file, ...r }) => r),
      null,
      2,
    ),
  );
  writeFileSync(sheetFile, verdictSheet(verdicts, unpaired, "End-to-end checkpoints, judged"));
  const cost = verdicts.reduce((sum, r) => sum + (r.cost ?? 0), 0);
  console.log(`${jsonFile}, ${sheetFile}${cost ? `; OpenRouter's cost: $${cost.toFixed(4)}` : ""}`);
  console.log(summary(verdicts, unpaired.length));
  if (verdicts.some((r) => r.verdict === "fail")) process.exit(1);
}

async function main() {
  const { values: opts, positionals: dirs } = parseArgs({
    allowPositionals: true,
    options: {
      model: { type: "string" },
      out: { type: "string" },
      only: { type: "string" },
      concurrency: { type: "string", default: "4" },
      "dry-run": { type: "boolean", default: false },
      manifest: { type: "string" },
      verdicts: { type: "string" },
    },
  });
  if (!dirs.length) {
    console.error(
      "usage: node tests/e2e/verify.mjs <out dir>... [--model <id>] [--out verify.html] [--only a,b] [--concurrency N] [--dry-run] [--manifest <file> | --verdicts <file>]",
    );
    process.exit(2);
  }
  const runs = dirs.map((dir) => ({ name: path.basename(path.resolve(dir)), dir, list: checkpointsOf(dir) }));
  const only = opts.only?.split(",").map((s) => s.trim()) ?? null;
  const names = new Set(runs.flatMap((r) => r.list.map((c) => c.scenario)));
  const { judged, unpaired, missing } = pair(runs, await expectationsOf(names), only);
  for (const m of missing) console.log(`  expected, not taken: ${m.scenario}/${m.label} (${m.run})`);
  for (const u of unpaired) console.log(`  no expectation: ${u.scenario}/${u.label} (${u.run})`);
  const wanted = opts.model ?? process.env.E2E_VERIFY_MODEL;

  if (opts["dry-run"]) {
    console.log(`model: ${wanted ?? "(none set: --model or E2E_VERIFY_MODEL)"}\n\nThe rubric:\n${RUBRIC}\n`);
    for (const j of judged) console.log(`${j.scenario}/${j.label} (${j.run}): ${j.expectation}`);
    console.log(`\n${judged.length} to judge, ${unpaired.length} without expectation`);
    return;
  }
  if (opts.manifest && opts.verdicts) {
    console.error("verify: --manifest and --verdicts are two steps; give one");
    process.exit(2);
  }
  if (opts.manifest) {
    mkdirSync(path.dirname(path.resolve(opts.manifest)), { recursive: true });
    writeFileSync(opts.manifest, JSON.stringify(manifestOf(judged), null, 2));
    console.log(`${opts.manifest}\n${judged.length} to judge, ${unpaired.length} without expectation`);
    return;
  }
  if (opts.verdicts) {
    let joined;
    try {
      joined = joinVerdicts(readFileSync(opts.verdicts, "utf8"), judged);
    } catch (e) {
      console.error(`verify: ${e.message}`);
      process.exit(2);
    }
    for (const f of joined.stray) console.log(`  verdict for no picture to judge: ${f}`);
    for (const r of joined.verdicts) {
      console.log(`  ${r.verdict}: ${r.scenario}/${r.label} (${r.run}): ${r.reason}`);
      for (const c of r.concerns) console.log(`      concern: ${c}`);
    }
    finish(joined.verdicts, unpaired, dirs, opts);
    return;
  }
  const key = process.env.OPENROUTER_API_KEY;
  if (!key) {
    console.error("verify: OPENROUTER_API_KEY isn't set (--dry-run shows what would be sent)");
    process.exit(2);
  }
  if (!wanted) {
    console.error('verify: no model: --model or E2E_VERIFY_MODEL, an id or a prefix ending in "*"');
    process.exit(2);
  }
  const list = await fetch(`${API}/models`, { signal: AbortSignal.timeout(60_000) }).then((r) => {
    if (!r.ok) throw new Error(`OpenRouter's model list: ${r.status}`);
    return r.json();
  });
  const model = pickModel(wanted, list.data ?? []);
  const perMillion = (p) => (Number.isFinite(p) ? `$${+(p * 1e6).toFixed(3)}` : "?");
  console.log(
    `model: ${model.id} (${perMillion(model.prompt)} in, ${perMillion(model.completion)} out, per million tokens)`,
  );

  const concurrency = Math.max(1, Number(opts.concurrency) || 4);
  let verdicts;
  try {
    verdicts = await pool(judged, concurrency, async (item) => {
      const r = await judge(item, model, key);
      console.log(`  ${r.verdict}: ${r.scenario}/${r.label} (${r.run}): ${r.reason}`);
      for (const c of r.concerns) console.log(`      concern: ${c}`);
      return r;
    });
  } catch (e) {
    console.error(`verify: ${e.message}`);
    process.exit(2);
  }

  finish(verdicts, unpaired, dirs, opts);
}

if (import.meta.main) await main();
