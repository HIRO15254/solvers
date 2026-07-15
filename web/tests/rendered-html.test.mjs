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
  assert.match(html, /<title>Solvers Lab — プリフロップ設定<\/title>/i);
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
