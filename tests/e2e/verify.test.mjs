// verify.mjs's own parts: checkpoints paired with expectations, the model's
// answer read, the model picked from OpenRouter's list, the summary line.

import { spawnSync } from "node:child_process";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import { checkpointsOf } from "./contact-sheet.mjs";
import { ANSWER_FORMAT, RUBRIC, messages, pair, parseVerdict, pickModel, summary } from "./verify.mjs";

const run = (name, list) => ({
  name,
  dir: `/out/${name}`,
  list: list.map(([scenario, label], i) => ({ file: `${i + 1}-${scenario}-${label}.png`, scenario, label })),
});

test("a run's index names only pictures in its own checkpoints folder", () => {
  // An out dir can come back from a runner: a name in its index that leads
  // elsewhere would have put any picture on this computer in the sheet,
  // and sent it to the model.
  const dir = mkdtempSync(path.join(os.tmpdir(), "verify-index-"));
  try {
    mkdirSync(path.join(dir, "checkpoints"));
    const entry = (file) => ({ file, scenario: "s", label: "l" });
    const files = ["01-s-l.png", "../../secret.png", "/etc/secret.png", "sub/02-s-l.png", "..", ".", "", 3];
    writeFileSync(path.join(dir, "checkpoints", "index.json"), JSON.stringify(files.map(entry)));
    assert.deepEqual(
      checkpointsOf(dir).map((c) => c.file),
      ["01-s-l.png"],
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("checkpoints with an expectation are judged, the rest listed", () => {
  const runs = [
    run("a", [
      ["settings", "settings"],
      ["settings", "settings-dark"],
      ["clip", "clip-bar"],
    ]),
  ];
  const expects = { settings: { settings: "The dialog.", "settings-help": "Help." } };
  const { judged, unpaired, missing } = pair(runs, expects);
  assert.deepEqual(judged, [
    {
      run: "a",
      scenario: "settings",
      label: "settings",
      file: path.join("/out/a", "checkpoints", "1-settings-settings.png"),
      expectation: "The dialog.",
    },
  ]);
  assert.deepEqual(
    unpaired.map((u) => `${u.scenario}/${u.label}`),
    ["settings/settings-dark", "clip/clip-bar"],
  );
  // settings ran without its Help checkpoint: said, so a renamed label isn't silently unjudged.
  assert.deepEqual(missing, [{ run: "a", scenario: "settings", label: "settings-help" }]);
});

test("each run's checkpoints are paired; --only keeps those scenarios", () => {
  const runs = [
    run("a", [["search", "search-or"]]),
    run("b", [
      ["search", "search-or"],
      ["edits", "edit-view"],
    ]),
  ];
  const expects = { search: { "search-or": "Chips." }, edits: { "edit-clip-bar": "A pill." } };
  const all = pair(runs, expects);
  assert.deepEqual(
    all.judged.map((j) => j.run),
    ["a", "b"],
  );
  assert.deepEqual(
    all.missing.map((m) => `${m.run}:${m.label}`),
    ["b:edit-clip-bar"],
  );
  const only = pair(runs, expects, ["edits"]);
  assert.equal(only.judged.length, 0);
  assert.deepEqual(
    only.unpaired.map((u) => u.label),
    ["edit-view"],
  );
  // A scenario that didn't run isn't missing its expectations.
  assert.deepEqual(pair([run("c", [["clip", "clip-bar"]])], expects).missing, []);
});

test("an answer is read with or without a fence and around prose", () => {
  const want = { verdict: "pass", reason: "The chips show.", concerns: [] };
  assert.deepEqual(parseVerdict('{"verdict":"pass","reason":"The chips show.","concerns":[]}'), want);
  assert.deepEqual(
    parseVerdict('```json\n{"verdict": "pass", "reason": "The chips show.", "concerns": []}\n```'),
    want,
  );
  assert.deepEqual(parseVerdict('Here it is: {"verdict":"PASS","reason":" The chips show. "} Done.'), want);
  assert.deepEqual(parseVerdict('{"verdict":"fail","reason":"No OR.","concerns":["text cut off", ""]}'), {
    verdict: "fail",
    reason: "No OR.",
    concerns: ["text cut off"],
  });
});

test("an answer of the wrong shape is refused", () => {
  assert.throws(() => parseVerdict("It looks fine."), /no JSON object/);
  assert.throws(() => parseVerdict('{"verdict":"maybe","reason":"x"}'), /verdict/);
  assert.throws(() => parseVerdict('{"verdict":"pass"}'), /no reason/);
  assert.throws(() => parseVerdict('{"verdict":"pass","reason":"x","concerns":"none"}'), /concerns/);
  assert.throws(() => parseVerdict('{"verdict":"pass", reason}'), SyntaxError);
});

test("the request: the rubric, then the expectation and the picture", () => {
  const [system, user] = messages("Two chips.", "QUJD");
  assert.equal(system.role, "system");
  assert.equal(system.content, RUBRIC);
  assert.match(RUBRIC, /Judge only what is visible/);
  assert.match(RUBRIC, /"verdict": "pass" \| "fail" \| "unsure"/);
  for (const word of ["broken", "cropped", "overlapping", "unreadable", "off-theme"])
    assert.match(RUBRIC, new RegExp(word));
  assert.deepEqual(user.content, [
    { type: "text", text: "Expectation: Two chips." },
    { type: "image_url", image_url: { url: "data:image/jpeg;base64,QUJD" } },
  ]);
});

test("a prefix picks the cheapest model with it that takes pictures", () => {
  const models = [
    { id: "vendor/small-2", pricing: { prompt: "0.000002", completion: "0.00001" } },
    { id: "vendor/small-1", pricing: { prompt: "0.0000008", completion: "0.000004" } },
    {
      id: "vendor/small-text",
      pricing: { prompt: "0.0000001", completion: "0.0000001" },
      architecture: { input_modalities: ["text"] },
    },
    { id: "vendor/small-1:batch", pricing: { prompt: "0.0000004", completion: "0.000002" } },
    { id: "other/tiny", pricing: { prompt: "0", completion: "0" } },
  ];
  assert.deepEqual(pickModel("vendor/small*", models), {
    id: "vendor/small-1",
    prompt: 0.0000008,
    completion: 0.000004,
  });
  assert.equal(pickModel("vendor/small-2", models).id, "vendor/small-2");
  assert.ok(Number.isNaN(pickModel("vendor/unlisted", models).prompt));
  assert.throws(() => pickModel("nobody/*", models), /no model/);
});

test("the summary line counts verdicts and tokens", () => {
  const verdicts = [
    { verdict: "pass", tokens: { prompt: 1000, completion: 50 } },
    { verdict: "fail", tokens: { prompt: 900, completion: 40 } },
    { verdict: "unsure", tokens: null },
  ];
  assert.equal(summary(verdicts, 7), "verify: 1 pass, 1 fail, 1 unsure, 7 without expectation; ~1990 tokens");
});

test("every scenario's expectations are one or two sentences, keyed by a label", async () => {
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "scenarios");
  let count = 0;
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".mjs"))) {
    const { default: scenario } = await import(pathToFileURL(path.join(dir, file)));
    for (const [label, text] of Object.entries(scenario.expects ?? {})) {
      count += 1;
      assert.match(label, /^[\w-]+$/, `${file}: label ${label}`);
      assert.equal(typeof text, "string", `${file}/${label}`);
      const sentences = text.split(/(?<=[.!?])\s+(?=[A-Z])/).length;
      assert.ok(sentences <= 2, `${file}/${label}: ${sentences} sentences`);
    }
  }
  assert.ok(count >= 10, `${count} expectations`);
});

// The agent path, through the command line: a real out dir with two pictures of
// the legend scenario (whose expectations are real), judged without a key.
const VERIFY = path.join(path.dirname(fileURLToPath(import.meta.url)), "verify.mjs");

function withOutDir(body) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "verify-agent-"));
  try {
    mkdirSync(path.join(dir, "checkpoints"));
    const labels = ["legend-vod", "legend-edit", "legend-other"];
    for (const label of labels) {
      const made = spawnSync(
        "ffmpeg",
        ["-loglevel", "error", "-f", "lavfi", "-i", "color=c=gray:s=64x48", "-frames:v", "1", "-y"].concat(
          path.join(dir, "checkpoints", `${label}.png`),
        ),
      );
      assert.equal(made.status, 0, "ffmpeg makes a picture");
    }
    writeFileSync(
      path.join(dir, "checkpoints", "index.json"),
      JSON.stringify(labels.map((label) => ({ file: `${label}.png`, scenario: "legend", label }))),
    );
    return body(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const verify = (...args) =>
  spawnSync(process.execPath, [VERIFY, ...args], { encoding: "utf8", env: { ...process.env, OPENROUTER_API_KEY: "" } });

test("--manifest writes the rubric, the answer format and the pairs, and judges nothing", () =>
  withOutDir((dir) => {
    const file = path.join(dir, "manifest.json");
    const r = verify(dir, "--manifest", file);
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, /2 to judge, 1 without expectation/);
    const manifest = JSON.parse(readFileSync(file, "utf8"));
    assert.equal(manifest.rubric, RUBRIC);
    assert.equal(manifest.answer, ANSWER_FORMAT);
    assert.deepEqual(
      manifest.pairs.map((p) => [p.scenario, p.label, p.file, p.outDir]),
      ["legend-vod", "legend-edit"].map((l) => ["legend", l, path.join(dir, "checkpoints", `${l}.png`), dir]),
    );
    for (const p of manifest.pairs) assert.ok(path.isAbsolute(p.file) && p.expectation.length > 20);
    // --only narrows it as it does the other paths.
    const none = path.join(dir, "none.json");
    assert.equal(verify(dir, "--manifest", none, "--only", "settings").status, 0);
    assert.deepEqual(JSON.parse(readFileSync(none, "utf8")).pairs, []);
  }));

test("--verdicts: a fail exits 1, an unmatched pair is unsure, the sheet lists the concerns", () =>
  withOutDir((dir) => {
    const lines = [
      { file: "legend-vod.png", verdict: "fail", reason: "The columns are missing.", concerns: ["text cut off"] },
      { file: "elsewhere.png", verdict: "pass", reason: "Not a picture here.", concerns: [] },
    ];
    const verdicts = path.join(dir, "verdicts.jsonl");
    writeFileSync(verdicts, lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
    const r = verify(dir, "--verdicts", verdicts);
    assert.equal(r.status, 1, r.stdout + r.stderr);
    assert.match(r.stdout, /verdict for no picture to judge: elsewhere.png/);
    assert.match(r.stdout, /verify: 0 pass, 1 fail, 1 unsure, 1 without expectation; ~0 tokens/);
    const json = JSON.parse(readFileSync(path.join(dir, "verify.json"), "utf8"));
    assert.deepEqual(
      json.map((v) => [v.label, v.verdict, v.reason]),
      [
        ["legend-vod", "fail", "The columns are missing."],
        ["legend-edit", "unsure", "no verdict"],
      ],
    );
    const html = readFileSync(path.join(dir, "verify.html"), "utf8");
    assert.match(html, /text cut off/);
    assert.ok(html.indexOf("legend-vod") < html.indexOf("legend-edit"), "fails first");
  }));

test("--verdicts matches an absolute path, passes exit 0, and refuses a line that isn't a verdict", () =>
  withOutDir((dir) => {
    const verdicts = path.join(dir, "verdicts.jsonl");
    const both = ["legend-vod", "legend-edit"].map((l, i) => ({
      file: i ? `${l}.png` : path.join(dir, "checkpoints", `${l}.png`),
      verdict: "pass",
      reason: "Fine.",
      concerns: [],
    }));
    writeFileSync(verdicts, both.map((l) => JSON.stringify(l)).join("\n"));
    const ok = verify(dir, "--verdicts", verdicts);
    assert.equal(ok.status, 0, ok.stdout + ok.stderr);
    assert.match(ok.stdout, /verify: 2 pass, 0 fail, 0 unsure/);
    writeFileSync(verdicts, '{"file":"legend-vod.png","verdict":"maybe","reason":"x"}\n');
    const bad = verify(dir, "--verdicts", verdicts);
    assert.equal(bad.status, 2);
    assert.match(bad.stderr, /line 1/);
  }));
