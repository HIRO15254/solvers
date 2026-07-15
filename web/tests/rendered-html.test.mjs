import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import test from "node:test";

async function render() {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}`);
  const { default: worker } = await import(workerUrl.href);

  return worker.fetch(
    new Request("http://localhost/", {
      headers: { accept: "text/html" },
    }),
    {
      ASSETS: {
        fetch: async () => new Response("Not found", { status: 404 }),
      },
    },
    {
      waitUntil() {},
      passThroughOnException() {},
    },
  );
}

test("server-renders the preflop setup workspace", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^text\/html\b/i);

  const html = await response.text();
  assert.match(html, /<html[^>]+lang="ja"/i);
  assert.match(html, /<title>Solvers Lab — HU \/ Multiway プリフロップソルバー<\/title>/i);
  assert.match(html, /og-multiway\.png/);
  assert.match(html, /HUプリフロップを/);
  assert.match(html, /PREFLOP WORKBENCH/);
  assert.match(html, /id="spot"/);
  assert.match(html, /id="tree"/);
  assert.match(html, /id="model"/);
  assert.match(html, /id="economics"/);
  assert.match(html, /id="run"/);
  assert.match(html, /100bb · Equity showdown/);
  assert.match(html, /10bb · Push \/ fold/);
  assert.match(html, /TOMLをコピー/);
  assert.doesNotMatch(html, /codex-preview|SkeletonPreview|react-loading-skeleton/);
});

test("removes disposable starter assets and keeps solver contracts", async () => {
  const [page, layout, packageJson, config] = await Promise.all([
    readFile(new URL("../app/page.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/layout.tsx", import.meta.url), "utf8"),
    readFile(new URL("../package.json", import.meta.url), "utf8"),
    readFile(new URL("../app/preflop-config.ts", import.meta.url), "utf8"),
  ]);

  assert.match(page, /\/v1\/health/);
  assert.match(page, /\/v1\/jobs/);
  assert.match(page, /targetAddressSpace:\s*"loopback"/);
  assert.match(layout, /lang="ja"/);
  assert.match(config, /kind = "preflop"/);
  assert.match(config, /export function validateSettings/);
  assert.match(config, /export function generateToml/);
  assert.match(config, /export function estimateSolve/);
  assert.doesNotMatch(packageJson, /react-loading-skeleton/);

  await assert.rejects(access(new URL("../app/_sites-preview", import.meta.url)));
});

test("keeps multiway v2 bridge and TOML contracts aligned", async () => {
  const [page, config, explorer] = await Promise.all([
    readFile(new URL("../app/page.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/preflop-config.ts", import.meta.url), "utf8"),
    readFile(new URL("../app/MultiwayResultExplorer.tsx", import.meta.url), "utf8"),
  ]);

  assert.match(page, /\/v2\/health/);
  assert.match(page, /\/v2\/validate/);
  assert.match(page, /"\/v2\/jobs"/);
  assert.match(page, /\/v2\/jobs\/.*\/strategies/);
  assert.match(page, /\/v2\/jobs\/.*\/cancel/);
  assert.match(page, /MultiwayResultExplorer/);
  assert.match(page, /MultiwaySeatOverrides/);
  assert.match(page, /className="[^"]*(?:cancel-button|danger-button)[^"]*"/);
  assert.match(page, /全席へ適用/);
  assert.match(page, /const terminalJob = await responseJson<BridgeJob>/);
  assert.match(page, /terminalJob\.resultUrl/);

  for (const presetId of [
    "multiway-9max-pushfold",
    "multiway-9max-mtt-icm",
    "multiway-6max-cash",
    "multiway-9max-research",
  ]) {
    assert.match(config, new RegExp(`id:\\s*"${presetId}"`));
  }
  for (const presetName of [
    "9-max · 10bb push / fold",
    "9-max · 20bb MTT + BBA + ICM",
    "6-max · 100bb cash",
    "9-max · 100bb research",
  ]) {
    assert.ok(config.includes(presetName), `missing preset: ${presetName}`);
  }

  assert.match(
    config,
    /9:\s*\["UTG",\s*"UTG\+1",\s*"MP",\s*"LJ",\s*"HJ",\s*"CO",\s*"BTN",\s*"SB",\s*"BB"\]/,
  );
  assert.match(config, /kind = "preflop-multiway"/);
  assert.match(config, /evaluation_samples = 32/);
  assert.match(config, /game\.abstraction\.active_opponent_buckets/);
  assert.match(config, /\[\[game\.seats\]\]/);
  assert.match(config, /"game\.seats\.betting"/);
  assert.match(config, /if \(seat\.betting\)/);
  assert.match(config, /cap = \$\{tomlFloat/);
  assert.doesNotMatch(config, /cap_bb/);
  assert.match(explorer, /13×13 preflop strategy/);
  assert.match(explorer, /Structured labels from the v2 strategy artifact/);
  assert.match(explorer, /Private recall path/);
  assert.match(explorer, /slice\(0, block\.key\.street\)/);
  assert.match(explorer, /bucket_path\[block\.key\.street\]/);
});

test("keeps dialogs, progress, approximation, and result exploration accessible", async () => {
  const [page, explorer, styles] = await Promise.all([
    readFile(new URL("../app/page.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/MultiwayResultExplorer.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/globals.css", import.meta.url), "utf8"),
  ]);

  assert.match(page, /role="dialog"/);
  assert.match(page, /aria-modal="true"/);
  assert.match(page, /modalRef\.current\.querySelectorAll/);
  assert.match(page, /lastFocusedRef\.current\?\.focus/);
  assert.match(page, /role="progressbar"/);
  assert.match(page, /aria-live="polite"/);
  assert.match(page, /approximation-notice/);
  assert.match(explorer, /className="approximation-notice" role="note"/);
  assert.match(explorer, /role="grid"/);
  assert.match(explorer, /role="gridcell"/);
  assert.match(explorer, /aria-selected=/);

  assert.match(styles, /\.result-explorer/);
  assert.match(styles, /\.strategy-grid/);
  assert.match(styles, /\.strategy-cell\[aria-selected="true"\]/);
  assert.match(styles, /\.(?:cancel-button|danger-button)/);
  assert.match(styles, /@media \(max-width: 620px\)/);
  assert.match(styles, /@media \(forced-colors: active\)/);
});
