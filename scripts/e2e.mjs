// P5.7 end-to-end test: drives the real ClipPro build through WebDriver (tauri-driver +
// msedgedriver in third_party/tools). Scenario: new project → import the mini corpus → wait for
// analysis → Cleanup preset → check selects in the store and the window → raise the score
// threshold with the keyboard → Selects only → close, reopen, compare.
//
// Prerequisites: `npm --prefix app/ui run build`, then
//   cargo build -p clippro-app --features tauri/custom-protocol
// and the corpus (testdata/gen). Run from the repo root in a shell that sourced scripts/env.sh:
//   node scripts/e2e.mjs [path/to/ClipPro.exe]
// Exit code 0 = pass. Temporary files go to target/tmp-e2e and are removed.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1")), "..");
const exe = path.resolve(process.argv[2] ?? path.join(root, "target/debug/ClipPro.exe"));
const driver = path.join(root, "third_party/tools/bin/tauri-driver.exe");
const native = path.join(root, "third_party/tools/msedgedriver.exe");
const corpus = path.join(root, "testdata/gen");
const tmp = path.join(root, "target/tmp-e2e");
const PORT = 4444;
const W3C_ELEMENT = "element-6066-11e4-a52e-4f735466cecc";

for (const [p, what] of [[exe, "the app build"], [driver, "tauri-driver"], [native, "msedgedriver"], [path.join(corpus, "tripod_static.mp4"), "the corpus"]]) {
  if (!existsSync(p)) {
    console.error(`missing ${what}: ${p}`);
    process.exit(2);
  }
}
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const base = `http://127.0.0.1:${PORT}`;
let session = null;

async function wd(method, url, body) {
  const r = await fetch(base + url, { method, headers: { "content-type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok || (j.value && j.value.error)) throw new Error(`${method} ${url}: ${JSON.stringify(j.value ?? j).slice(0, 400)}`);
  return j.value;
}
const exec = (script, args = []) => wd("POST", `/session/${session}/execute/sync`, { script, args });
const execAsync = (script, args = []) => wd("POST", `/session/${session}/execute/async`, { script, args });
const find = (css) => wd("POST", `/session/${session}/element`, { using: "css selector", value: css }).then((e) => e[W3C_ELEMENT] ?? Object.values(e ?? {})[0]);

async function until(what, fn, timeoutMs = 120_000) {
  const t0 = Date.now();
  for (;;) {
    const v = await fn().catch(() => null);
    if (v) return v;
    if (Date.now() - t0 > timeoutMs) throw new Error(`timed out waiting for ${what}`);
    await sleep(500);
  }
}

function check(cond, msg) {
  if (!cond) throw new Error(`check failed: ${msg}`);
  console.log(`  ✓ ${msg}`);
}

/** Calls a store action and waits for its promise. */
const act = (name, ...args) =>
  execAsync(
    `const done = arguments[arguments.length - 1]; const [n, a] = [arguments[0], arguments[1]];
     Promise.resolve(window.__clippro.store.getState()[n](...a)).then(() => done(null), (e) => done(String(e)));`,
    [name, args],
  ).then((err) => {
    if (err) throw new Error(`${name}: ${err}`);
  });

const state = () =>
  exec(`const s = window.__clippro.store.getState();
        return { project: s.project && s.project.name, clips: s.clips.map((c) => ({ name: c.name, state: c.state })),
                 count: s.scored ? s.scored.summary.count : null, perClip: s.scored ? s.scored.clips.map((c) => [c.clip, c.segments.length]) : [],
                 ids: s.clips.map((c) => [c.name, c.clip]), lo: s.settings ? s.settings.score_lo : null, preset: s.settings ? s.settings.preset_name : null,
                 selectsOnly: s.selectsOnly };`);

const env = { ...process.env, CLIPPRO_E2E: "1" };
const drv = spawn(driver, ["--port", String(PORT), "--native-port", String(PORT + 2), "--native-driver", native], { env, stdio: ["ignore", "inherit", "inherit"] });
let code = 1;
try {
  await until("tauri-driver", () => fetch(base + "/status").then((r) => r.ok), 20_000);
  const s = await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: exe } } } });
  session = s.sessionId;
  console.log("session", session);
  await until("the app's E2E hook", () => exec("return !!window.__clippro"), 30_000);

  console.log("1. new project and import");
  const proj = path.join(tmp, "E2E.clippro");
  await act("newProject", proj, "E2E");
  const files = ["tripod_static.mp4", "solid_black.mp4", "handheld_shake.mp4", "blur_ramp.mp4"].map((f) => path.join(corpus, f));
  await act("importPaths", files);
  await until("four clips listed", async () => (await state()).clips.length === 4);
  const listed = await exec(`return [...document.querySelectorAll('[role="option"]')].map((e) => e.textContent);`);
  check(["tripod_static", "solid_black"].every((n) => listed.some((t) => t.includes(n))), "the clip list shows the imported clips");

  console.log("2. analysis");
  await until("analysis to finish", async () => {
    const st = await state();
    return st.clips.length === 4 && st.clips.every((c) => c.state === "done" || c.state === "failed");
  }, 300_000);
  const st1 = await state();
  check(st1.clips.every((c) => c.state === "done"), "all four clips analysed");

  console.log("3. Cleanup preset");
  await act("setPreset", "Cleanup");
  await until("scores", async () => (await state()).preset === "Cleanup" && (await state()).count !== null);
  const st2 = await state();
  const idOf = (n) => st2.ids.find((x) => x[0] === n)[1];
  const segs = (st, n) => (st.perClip.find((c) => c[0] === idOf(n)) ?? [0, 0])[1];
  check(segs(st2, "tripod_static") >= 1, "tripod_static has a select");
  check(segs(st2, "solid_black") === 0, "solid_black has none");
  const chartText = await exec(`return document.body.textContent;`);
  check(chartText.includes(`${st2.count} select`), `the window reports ${st2.count} selects`);

  console.log("4. raise the threshold with the keyboard");
  const thumb = await find('[role="slider"][aria-label="Score threshold minimum"]');
  await wd("POST", `/session/${session}/element/${thumb}/click`, {});
  await wd("POST", `/session/${session}/element/${thumb}/value`, { text: "".repeat(10) }); // ArrowRight × 10
  await until("re-scoring", async () => (await state()).lo > st2.lo);
  const st3 = await state();
  check(st3.lo > st2.lo, `threshold moved ${st2.lo.toFixed(2)} → ${st3.lo.toFixed(2)}`);
  check(st3.count <= st2.count, `fewer or equal selects (${st2.count} → ${st3.count})`);

  console.log("5. selects only");
  const btn = await find('button[aria-label="Selects only"]');
  await wd("POST", `/session/${session}/element/${btn}/click`, {});
  await until("selects-only on", async () => (await state()).selectsOnly);
  check(true, "Selects only is on");

  console.log("6. close, reopen, compare");
  await act("closeProject");
  await until("project closed", async () => !(await state()).project);
  await act("openProject", proj);
  await until("project reopened with scores", async () => {
    const st = await state();
    return st.project === "E2E" && st.count !== null && st.clips.every((c) => c.state === "done");
  }, 120_000);
  const st4 = await state();
  check(Math.abs(st4.lo - st3.lo) < 1e-6, "the tuning came back with the project");
  check(st4.count === st3.count, `the same ${st4.count} selects after reopening, with no re-analysis`);
  console.log("PASS");
  code = 0;
} catch (e) {
  console.error("FAIL:", e.message);
} finally {
  if (session) await wd("DELETE", `/session/${session}`).catch(() => {});
  drv.kill();
  await sleep(500);
  rmSync(tmp, { recursive: true, force: true });
}
process.exit(code);
