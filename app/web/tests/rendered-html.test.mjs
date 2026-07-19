import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

const MULTIWAY_PRESET_IDS = [
  "multiway-9max-pushfold",
  "multiway-9max-mtt-icm",
  "multiway-6max-cash",
  "multiway-9max-research",
];

let preflopConfigModule;

async function loadPreflopConfig() {
  if (!preflopConfigModule) {
    preflopConfigModule = (async () => {
      const source = await readFile(
        new URL("../app/preflop-config.ts", import.meta.url),
        "utf8",
      );
      const transpiled = ts.transpileModule(source, {
        compilerOptions: {
          module: ts.ModuleKind.ESNext,
          target: ts.ScriptTarget.ES2022,
        },
        fileName: "preflop-config.ts",
        reportDiagnostics: true,
      });
      assert.deepEqual(transpiled.diagnostics ?? [], []);
      const moduleUrl = `data:text/javascript;base64,${Buffer.from(
        transpiled.outputText,
        "utf8",
      ).toString("base64")}`;
      return import(moduleUrl);
    })();
  }
  return preflopConfigModule;
}

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
  assert.match(html, /id="validation-summary"/);
  assert.match(html, /role="status"/);
  assert.match(html, /aria-describedby="validation-summary"/);
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
  const [page, config, explorer, overrides] = await Promise.all([
    readFile(new URL("../app/page.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/preflop-config.ts", import.meta.url), "utf8"),
    readFile(new URL("../app/MultiwayResultExplorer.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/MultiwaySeatOverrides.tsx", import.meta.url), "utf8"),
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
  assert.match(page, /resumeCheckpointUrl:\s*settings\.resumeCheckpoint\.trim\(\)/);
  assert.match(page, /optional managed Bridge URL/);
  assert.match(page, /\/v2\/jobs\/\{id\}\/checkpoint/);
  assert.match(page, /profileEv/);
  assert.match(page, /averagePositiveRegret/);
  assert.match(page, /strategyDriftL1/);
  assert.doesNotMatch(page, /exploitability/);
  assert.ok(
    config.includes("!/^\\/v2\\/jobs\\/[0-9a-fA-F]{32}\\/checkpoint$/.test"),
    "resume validation must accept only a managed 32-hex Bridge checkpoint URL",
  );

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
  assert.match(config, /isolate_sizes = \$\{tomlObjectList/);
  assert.match(config, /streetBetting\.betSizes/);
  assert.match(config, /streetBetting\.raiseSizes/);
  assert.match(config, /streetBetting\.maxAggressiveActions/);
  assert.match(config, /streetBetting\.includeAllin/);
  assert.match(config, /max_memory_bytes = \$\{tomlInteger\(settings\.maxMemoryBytes\)\}/);
  assert.match(config, /evaluation_samples = \$\{tomlInteger\(settings\.evaluationSamples\)\}/);
  assert.match(config, /evaluation_cadence = \$\{tomlInteger\(settings\.evaluationCadence\)\}/);
  assert.ok(
    config.indexOf("evaluation_cadence = ${tomlInteger(settings.evaluationCadence)}") <
      config.indexOf("if (settings.checkpointEvery > 0)"),
    "evaluation cadence must be emitted independently of checkpoint cadence",
  );
  assert.match(config, /200-bucket profiles/);
  assert.doesNotMatch(config, /96-bucket profiles/);
  assert.match(config, /game\.abstraction\.active_opponent_buckets/);
  assert.match(config, /\[\[game\.seats\]\]/);
  assert.match(config, /"game\.seats\.betting"/);
  assert.match(config, /if \(seat\.betting\)/);
  assert.match(config, /cap = \$\{tomlFloat/);
  assert.doesNotMatch(config, /cap_bb/);
  assert.match(overrides, /isolateSizesBb/);
  assert.match(overrides, /"betSizes", "raiseSizes"/);
  assert.match(overrides, /maxAggressiveActions/);
  assert.match(overrides, /includeAllin/);
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
  assert.match(page, /VALIDATION_SUMMARY_ID = "validation-summary"/);
  assert.match(page, /VALIDATION_MESSAGES_ID = "validation-messages"/);
  assert.match(page, /role=\{validationErrors\.length \? "alert" : "status"\}/);
  assert.match(page, /const fieldValidationProps = useCallback/);
  assert.match(page, /"aria-invalid": true/);
  assert.match(page, /"aria-errormessage": `validation-\$\{field\}`/);
  assert.match(page, /aria-describedby=\{VALIDATION_SUMMARY_ID\}/);
  assert.match(page, /\.\.\.fieldValidationProps/);
  assert.doesNotMatch(page, /validationControlProps/);
  assert.match(page, /interface SubmittedJobSnapshot/);
  assert.match(page, /setSubmittedJob\(submittedSnapshot\)/);
  assert.match(page, /jobMode === "multiway"/);
  assert.match(page, /positions=\{jobPositions\}/);
  assert.match(page, /className="icm-readback-grid"/);
  assert.match(styles, /\.icm-readback-table/);
  assert.match(styles, /\.icm-readback-grid/);
  assert.doesNotMatch(page.match(/function setWorkbenchMode[\s\S]*?\n  }/)?.[0] ?? "", /setJob\(/);
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
  assert.match(styles, /overflow-x:\s*clip/);
  assert.match(styles, /@media \(forced-colors: active\)/);
  assert.match(styles, /\.settings-stack\s*\{[^}]*min-width:\s*0/);
  assert.match(styles, /\.settings-card\s*\{[^}]*min-width:\s*0/);
  assert.match(styles, /\.multiway-spot,[^}]*min-width:\s*0/);
  assert.match(styles, /\.table-scroll\s*\{[^}]*max-width:\s*100%/);
});

test("keeps every multiway preset byte-identical to its canonical TOML snapshot", async () => {
  const { PRESETS, generateToml } = await loadPreflopConfig();
  const multiwayPresets = PRESETS.filter(
    (preset) => preset.settings.mode === "multiway",
  );
  assert.deepEqual(
    multiwayPresets.map((preset) => preset.id),
    MULTIWAY_PRESET_IDS,
  );

  for (const preset of multiwayPresets) {
    const expected = await readFile(
      new URL(`../presets/${preset.id}.toml`, import.meta.url),
    );
    const actual = Buffer.from(generateToml(preset.settings), "utf8");
    assert.deepEqual(
      actual,
      expected,
      `${preset.id} TOML differs from its canonical snapshot`,
    );
  }
});

test("keeps raw sizing drafts and seat navigation semantics", async () => {
  const [commaInput, seatTabs, overrides] = await Promise.all([
    readFile(new URL("../app/CommaListInput.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/MultiwaySeatTabs.tsx", import.meta.url), "utf8"),
    readFile(new URL("../app/MultiwaySeatOverrides.tsx", import.meta.url), "utf8"),
  ]);
  assert.match(commaInput, /const \[draft, setDraft\] = useState/);
  assert.match(commaInput, /onBlur=\{commit\}/);
  assert.match(commaInput, /event\.key === "Enter"/);
  assert.match(commaInput, /key=\{serialized\}/);
  assert.match(overrides, /<CommaListInput/);
  assert.match(overrides, /validationProps/);
  assert.match(seatTabs, /role="toolbar"/);
  assert.match(seatTabs, /aria-pressed=/);
  assert.doesNotMatch(seatTabs, /role="tablist"|role="tab"/);
});

test("validates Bridge v2 limits and preserves incomplete sizing drafts", async () => {
  const { PRESETS, parseSizingListDraft, validateSettings } =
    await loadPreflopConfig();
  const preset = PRESETS.find(
    (candidate) => candidate.id === "multiway-9max-pushfold",
  );
  assert.ok(preset);

  assert.deepEqual(parseSizingListDraft("0.5,"), [0.5]);
  assert.deepEqual(parseSizingListDraft("0."), [0]);
  assert.deepEqual(parseSizingListDraft(".75, 1.25"), [0.75, 1.25]);

  const errorsAfter = (mutate) => {
    const settings = structuredClone(preset.settings);
    mutate(settings);
    return validateSettings(settings);
  };
  assert.equal(
    errorsAfter((settings) => { settings.sbBb = 0.001; })
      .some((error) => error.includes("Small blind")),
    false,
  );
  assert.equal(
    errorsAfter((settings) => { settings.sbBb = 0.999; })
      .some((error) => error.includes("Small blind")),
    false,
  );
  assert.ok(
    errorsAfter((settings) => { settings.sbBb = 0.0001; })
      .some((error) => error.includes("Small blind")),
  );
  assert.ok(
    errorsAfter((settings) => { settings.seats[0].stackBb = 1000.1; })
      .some((error) => error.includes("may not exceed 1000bb")),
  );
  assert.ok(
    errorsAfter((settings) => { settings.seats[0].range = "AA,".repeat(1400); })
      .some((error) => error.includes("4096-byte limit")),
  );
  assert.ok(
    errorsAfter((settings) => { settings.isolateSizesBb = Array(17).fill(3); })
      .some((error) => error.includes("at most 16 sizes")),
  );
  assert.ok(
    errorsAfter((settings) => { settings.maxRaises = 17; })
      .some((error) => error.includes("may not exceed 16")),
  );
  assert.ok(
    errorsAfter((settings) => { settings.bucketProfiles[0].flop = 4097; })
      .some((error) => error.includes("may not exceed 4096")),
  );

  const icmPreset = PRESETS.find(
    (candidate) => candidate.id === "multiway-9max-mtt-icm",
  );
  assert.ok(icmPreset);
  const flatIcm = structuredClone(icmPreset.settings);
  flatIcm.payoutsText = "0";
  assert.ok(
    validateSettings(flatIcm).some((error) =>
      error.includes("at least two distinct amounts"),
    ),
  );

  const tenThousandPlayerIcm = structuredClone(icmPreset.settings);
  tenThousandPlayerIcm.outsideStacksText = Array(10_000 - tenThousandPlayerIcm.tableSize)
    .fill("20")
    .join(",");
  tenThousandPlayerIcm.payoutsText = "100,0";
  assert.equal(
    validateSettings(tenThousandPlayerIcm).some((error) =>
      error.includes("ICM supports at most"),
    ),
    false,
  );
  tenThousandPlayerIcm.outsideStacksText += ",20";
  assert.ok(
    validateSettings(tenThousandPlayerIcm).some((error) =>
      error.includes("ICM supports at most 10000"),
    ),
  );
});
