"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  DEFAULT_SETTINGS,
  PRESETS,
  estimateSolve,
  generateToml,
  validateSettings,
  type PreflopSettings,
  type RakeMode,
  type ScheduleKind,
  type StorageKind,
} from "./preflop-config";

const RANKS = ["A", "K", "Q", "J", "T", "9", "8", "7", "6", "5", "4", "3", "2"];
const HANDS = RANKS.flatMap((rowRank, row) =>
  RANKS.map((columnRank, column) => {
    if (row === column) return `${rowRank}${columnRank}`;
    return row < column
      ? `${rowRank}${columnRank}s`
      : `${columnRank}${rowRank}o`;
  }),
);

const DEFAULT_BRIDGE_URL = "http://127.0.0.1:38127";

type RangeSeat = "sb" | "bb";
type EngineState = "offline" | "checking" | "online";
type ModalKind = "connection" | "range" | "review" | "result" | null;

interface BridgeProgress {
  iteration: number;
  elapsedSecs: number;
  explP0: number;
  explP1: number;
  nashConv: number;
}

interface BridgeJob {
  id: string;
  status: "running" | "succeeded" | "failed";
  progress?: BridgeProgress | null;
  resultUrl?: string | null;
  error?: { code: string; message: string } | null;
}

interface BridgeHealth {
  service: string;
  version: string;
  apiVersion: number;
  busy: boolean;
}

const STEPS = [
  { id: "spot", number: "01", title: "スポット", note: "Stack & ranges" },
  { id: "tree", number: "02", title: "ベットツリー", note: "Actions & sizes" },
  { id: "model", number: "03", title: "継続モデル", note: "Postflop model" },
  { id: "economics", number: "04", title: "レーキ", note: "Economics" },
  { id: "run", number: "05", title: "実行精度", note: "Accuracy & run" },
];

function cloneSettings(settings: PreflopSettings): PreflopSettings {
  return {
    ...settings,
    openSizesBb: [...settings.openSizesBb],
    raiseFactors: settings.raiseFactors.map((level) => [...level]),
    equityRealization: { ...settings.equityRealization },
    buckets: { ...settings.buckets },
    postflopBetSizes: {
      flop: [...settings.postflopBetSizes.flop],
      turn: [...settings.postflopBetSizes.turn],
      river: [...settings.postflopBetSizes.river],
    },
  };
}

function handSetFromRange(range: string): Set<string> {
  if (range.trim() === "") return new Set(HANDS);
  const values = range
    .split(",")
    .map((entry) => entry.trim().split(":")[0])
    .filter((entry) => HANDS.includes(entry));
  return new Set(values);
}

function rangeFromHandSet(hands: Set<string>): string {
  if (hands.size === HANDS.length) return "";
  return HANDS.filter((hand) => hands.has(hand)).join(",");
}

function percentOfRange(hands: Set<string>): number {
  const combos = HANDS.reduce((sum, hand) => {
    if (!hands.has(hand)) return sum;
    if (hand.length === 2) return sum + 6;
    return sum + (hand.endsWith("s") ? 4 : 12);
  }, 0);
  return (combos / 1326) * 100;
}

function numberValue(raw: string): number {
  if (raw.trim() === "") return 0;
  return Number(raw);
}

function translateValidation(error: string): string {
  if (error.includes("Effective stack")) return "実効スタックは 1bb より大きくしてください。";
  if (error.includes("Small blind")) return "SB は 0.1〜0.9bb の範囲で指定してください。";
  if (error.includes("Open size")) return "オープンサイズは 2bb 以上で指定してください。";
  if (error.includes("Raise level")) return "リレイズ倍率は 1.0 より大きくしてください。";
  if (error.includes("Maximum raises")) return "最大レイズ回数を見直してください。";
  if (error.includes("Iterations")) return "反復回数は 1 以上の整数にしてください。";
  if (error.includes("Check cadence")) return "収束チェック間隔を 1 以上にしてください。";
  if (error.includes("buckets")) return "各ストリートのバケット数を 1 以上にしてください。";
  if (error.includes("range")) return "レンジに有効な169クラスを1つ以上含めてください。";
  if (error.includes("Rake")) return "レーキ率と上限を見直してください。";
  return error;
}

function SectionHeading({
  number,
  title,
  description,
  badge,
}: {
  number: string;
  title: string;
  description: string;
  badge?: string;
}) {
  return (
    <div className="card-heading">
      <span className="section-number" aria-hidden="true">
        {number}
      </span>
      <div className="section-copy">
        <h2>{title}</h2>
        <p>{description}</p>
      </div>
      {badge ? <span className="status-badge">{badge}</span> : null}
    </div>
  );
}

function Toggle({
  pressed,
  onPressedChange,
  label,
  description,
}: {
  pressed: boolean;
  onPressedChange: (next: boolean) => void;
  label: string;
  description: string;
}) {
  return (
    <button
      type="button"
      className="toggle"
      aria-pressed={pressed}
      onClick={() => onPressedChange(!pressed)}
    >
      <span className="toggle-copy">
        <strong>{label}</strong>
        <small>{description}</small>
      </span>
      <span className="toggle-track" aria-hidden="true" />
    </button>
  );
}

function loopbackFetch(url: string, init: RequestInit = {}): Promise<Response> {
  return fetch(url, {
    ...init,
    targetAddressSpace: "loopback",
  } as RequestInit & { targetAddressSpace: "loopback" });
}

async function responseJson<T>(response: Response): Promise<T> {
  const payload = (await response.json()) as T & {
    error?: { code?: string; message?: string };
  };
  if (!response.ok) {
    throw new Error(payload.error?.message ?? `HTTP ${response.status}`);
  }
  return payload;
}

export default function Home() {
  const [settings, setSettings] = useState<PreflopSettings>(() =>
    cloneSettings(DEFAULT_SETTINGS),
  );
  const [activePreset, setActivePreset] = useState(PRESETS[0]?.id ?? "custom");
  const [activeStep, setActiveStep] = useState("spot");
  const [modal, setModal] = useState<ModalKind>(null);
  const [rangeSeat, setRangeSeat] = useState<RangeSeat>("sb");
  const [rangeFocus, setRangeFocus] = useState(0);
  const [ranges, setRanges] = useState<Record<RangeSeat, Set<string>>>(() => ({
    sb: handSetFromRange(DEFAULT_SETTINGS.sbRange),
    bb: handSetFromRange(DEFAULT_SETTINGS.bbRange),
  }));
  const [newOpenSize, setNewOpenSize] = useState("3.0");
  const [toast, setToast] = useState("");
  const [bridgeUrl, setBridgeUrl] = useState(() =>
    typeof window === "undefined"
      ? DEFAULT_BRIDGE_URL
      : sessionStorage.getItem("solvers.bridgeUrl") ?? DEFAULT_BRIDGE_URL,
  );
  const [bridgeToken, setBridgeToken] = useState(() =>
    typeof window === "undefined"
      ? ""
      : sessionStorage.getItem("solvers.bridgeToken") ?? "",
  );
  const [engineState, setEngineState] = useState<EngineState>("offline");
  const [engineBusy, setEngineBusy] = useState(false);
  const [engineVersion, setEngineVersion] = useState("");
  const [job, setJob] = useState<BridgeJob | null>(null);
  const [resultJson, setResultJson] = useState("");

  const validationErrors = useMemo(() => validateSettings(settings), [settings]);
  const estimate = useMemo(() => estimateSolve(settings), [settings]);
  const toml = useMemo(() => generateToml(settings), [settings]);

  const update = useCallback(
    <K extends keyof PreflopSettings>(key: K, value: PreflopSettings[K]) => {
      setSettings((current) => ({ ...current, [key]: value }));
      setActivePreset("custom");
    },
    [],
  );

  const showToast = useCallback((message: string) => {
    setToast(message);
    window.setTimeout(() => setToast(""), 3000);
  }, []);

  const authenticatedFetch = useCallback(
    (path: string, init: RequestInit = {}) => {
      const url = `${bridgeUrl.replace(/\/$/, "")}${path}`;
      return loopbackFetch(url, {
        ...init,
        headers: {
          Authorization: `Bearer ${bridgeToken}`,
          ...(init.body ? { "Content-Type": "application/json" } : {}),
          ...init.headers,
        },
      });
    },
    [bridgeToken, bridgeUrl],
  );

  const connectEngine = useCallback(async () => {
    if (!bridgeToken.trim()) {
      showToast("ターミナルに表示された接続トークンを入力してください。");
      return;
    }
    setEngineState("checking");
    try {
      const response = await authenticatedFetch("/v1/health");
      const health = await responseJson<BridgeHealth>(response);
      if (health.service !== "solvers" || health.apiVersion !== 1) {
        throw new Error("互換性のないローカルサービスです。");
      }
      setEngineState("online");
      setEngineBusy(health.busy);
      setEngineVersion(health.version);
      sessionStorage.setItem("solvers.bridgeUrl", bridgeUrl);
      sessionStorage.setItem("solvers.bridgeToken", bridgeToken);
      setModal(null);
      showToast("ローカルのPFソルバーへ接続しました。");
    } catch (error) {
      setEngineState("offline");
      showToast(error instanceof Error ? error.message : "接続できませんでした。");
    }
  }, [authenticatedFetch, bridgeToken, bridgeUrl, showToast]);

  const fetchResult = useCallback(
    async (resultUrl: string) => {
      const path = resultUrl.startsWith("http")
        ? resultUrl.replace(bridgeUrl.replace(/\/$/, ""), "")
        : resultUrl;
      const response = await authenticatedFetch(path);
      if (!response.ok) throw new Error("解析結果を取得できませんでした。");
      const text = await response.text();
      setResultJson(JSON.stringify(JSON.parse(text), null, 2));
      setModal("result");
    },
    [authenticatedFetch, bridgeUrl],
  );

  const refreshJob = useCallback(async () => {
    if (!job || job.status !== "running") return;
    try {
      const response = await authenticatedFetch(`/v1/jobs/${encodeURIComponent(job.id)}`);
      const nextJob = await responseJson<BridgeJob>(response);
      setJob(nextJob);
      if (nextJob.status === "succeeded" && nextJob.resultUrl) {
        setEngineBusy(false);
        showToast("解析が完了しました。");
        await fetchResult(nextJob.resultUrl);
      } else if (nextJob.status === "failed") {
        setEngineBusy(false);
        showToast(nextJob.error?.message ?? "解析に失敗しました。");
      }
    } catch {
      setEngineState("offline");
    }
  }, [authenticatedFetch, fetchResult, job, showToast]);

  useEffect(() => {
    if (!job || job.status !== "running") return;
    const timer = window.setInterval(() => void refreshJob(), 1000);
    return () => window.clearInterval(timer);
  }, [job, refreshJob]);

  useEffect(() => {
    if (!modal) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setModal(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [modal]);

  function applyPreset(id: string) {
    const preset = PRESETS.find((item) => item.id === id);
    if (!preset) return;
    const next = cloneSettings(preset.settings);
    setSettings(next);
    setRanges({
      sb: handSetFromRange(next.sbRange),
      bb: handSetFromRange(next.bbRange),
    });
    setActivePreset(id);
    showToast(`${preset.name} を適用しました。`);
  }

  function goToStep(id: string) {
    setActiveStep(id);
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  function setRange(next: Set<string>) {
    setRanges((current) => ({ ...current, [rangeSeat]: next }));
  }

  function toggleHand(hand: string) {
    const next = new Set(ranges[rangeSeat]);
    if (next.has(hand)) next.delete(hand);
    else next.add(hand);
    setRange(next);
  }

  function saveRanges() {
    setSettings((current) => ({
      ...current,
      sbRange: rangeFromHandSet(ranges.sb),
      bbRange: rangeFromHandSet(ranges.bb),
    }));
    setActivePreset("custom");
    setModal(null);
    showToast("開始レンジを更新しました。");
  }

  function onRangeKeyDown(event: React.KeyboardEvent<HTMLButtonElement>, index: number) {
    let next = index;
    if (event.key === "ArrowRight") next = Math.min(HANDS.length - 1, index + 1);
    if (event.key === "ArrowLeft") next = Math.max(0, index - 1);
    if (event.key === "ArrowDown") next = Math.min(HANDS.length - 1, index + 13);
    if (event.key === "ArrowUp") next = Math.max(0, index - 13);
    if (next === index) return;
    event.preventDefault();
    setRangeFocus(next);
    window.requestAnimationFrame(() => {
      document.getElementById(`range-${next}`)?.focus();
    });
  }

  function addOpenSize() {
    const value = numberValue(newOpenSize);
    if (!Number.isFinite(value) || value <= 0) {
      showToast("有効なオープンサイズを入力してください。");
      return;
    }
    update("openSizesBb", [...new Set([...settings.openSizesBb, value])].sort((a, b) => a - b));
  }

  function setRaiseFactor(index: number, value: number) {
    const next = settings.raiseFactors.map((level) => [...level]);
    while (next.length <= index) next.push([index === 0 ? 3 : 2.5]);
    next[index] = [value];
    update("raiseFactors", next);
  }

  function setMaximumRaises(value: number) {
    if (value > 1 && settings.raiseFactors.length === 0) {
      setSettings((current) => ({
        ...current,
        maxRaises: value,
        raiseFactors: [[3], [2.5]],
      }));
      setActivePreset("custom");
      return;
    }
    update("maxRaises", value);
  }

  async function copyToml() {
    try {
      await navigator.clipboard.writeText(toml);
      showToast("TOMLをクリップボードへコピーしました。");
    } catch {
      setModal("review");
    }
  }

  function downloadText(filename: string, content: string, type: string) {
    const url = URL.createObjectURL(new Blob([content], { type }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = filename;
    anchor.click();
    URL.revokeObjectURL(url);
  }

  async function startSolve() {
    if (validationErrors.length > 0) return;
    if (settings.postflopModel === "bucketed") {
      setModal("review");
      showToast("BucketedはTOMLを書き出し、CLIから実行できます。");
      return;
    }
    if (engineState !== "online") {
      setModal("connection");
      return;
    }
    try {
      setEngineBusy(true);
      const response = await authenticatedFetch("/v1/jobs", {
        method: "POST",
        body: JSON.stringify({ configToml: toml }),
      });
      const nextJob = await responseJson<BridgeJob>(response);
      setJob(nextJob);
      showToast("PF解析を開始しました。");
    } catch (error) {
      setEngineBusy(false);
      showToast(error instanceof Error ? error.message : "解析を開始できませんでした。");
    }
  }

  const primaryLabel =
    job?.status === "running"
      ? "解析中…"
      : settings.postflopModel === "bucketed"
        ? "設定を確認"
        : engineState === "online"
          ? "この設定で解析を開始"
          : "ソルバーへ接続";

  return (
    <main className="app-shell">
      <header className="topbar">
        <a className="brand" href="#spot" aria-label="Solvers Lab ホーム">
          <span className="brand-mark" aria-hidden="true">SL</span>
          SOLVERS LAB
        </a>
        <div className="mode-switch" role="tablist" aria-label="解析モード">
          <button className="mode-button" role="tab" aria-selected="true" type="button">
            PREFLOP
          </button>
          <button className="mode-button" role="tab" aria-selected="false" type="button" disabled>
            POSTFLOP
          </button>
        </div>
        <div className="top-status" aria-live="polite">
          <span className={`status-dot ${engineState}`} aria-hidden="true" />
          <span>
            {engineState === "online"
              ? `ENGINE ${engineVersion || "ONLINE"}`
              : engineState === "checking"
                ? "CONNECTING"
                : "ENGINE OFFLINE"}
          </span>
        </div>
      </header>

      <div className="workspace">
        <nav className="step-nav" aria-label="設定セクション">
          <p className="nav-kicker">Solve setup</p>
          <ol className="step-list">
            {STEPS.map((step) => (
              <li key={step.id}>
                <button
                  type="button"
                  className={`step-item ${activeStep === step.id ? "active" : ""}`}
                  onClick={() => goToStep(step.id)}
                  aria-current={activeStep === step.id ? "step" : undefined}
                >
                  <span className="step-index">{step.number}</span>
                  <span className="step-copy">
                    <strong>{step.title}</strong>
                    <small>{step.note}</small>
                  </span>
                </button>
              </li>
            ))}
          </ol>
          <div className="preset-mini">
            <div className="preset-mini-label">
              CURRENT
              <span>{activePreset === "custom" ? "CUSTOM" : "PRESET"}</span>
            </div>
            <strong>
              {activePreset === "custom"
                ? `${settings.effectiveStackBb}bb カスタム`
                : PRESETS.find((preset) => preset.id === activePreset)?.name}
            </strong>
            <p>変更は右側の見積りとTOMLへ即時反映されます。</p>
          </div>
        </nav>

        <div className="content-column">
          <header className="page-heading">
            <p className="eyebrow">
              <span className="eyebrow-mark" aria-hidden="true" />
              PREFLOP WORKBENCH
            </p>
            <div className="title-row">
              <h1>
                HUプリフロップを、<span>迷わず設計。</span>
              </h1>
            </div>
            <p className="lead">
              スポット、アクション、継続モデルを一つの流れで設定。入力中もツリー規模と計算負荷を確認でき、そのままローカルのPFソルバーへ渡せます。
            </p>
            <div className="preset-row" aria-label="プリフロッププリセット">
              {PRESETS.map((preset) => (
                <button
                  key={preset.id}
                  type="button"
                  className="preset-card"
                  aria-pressed={activePreset === preset.id}
                  onClick={() => applyPreset(preset.id)}
                >
                  <span className="preset-check" aria-hidden="true">✓</span>
                  <strong>{preset.name}</strong>
                  <small>{preset.description}</small>
                </button>
              ))}
            </div>
          </header>

          <div className="settings-stack">
            <section className="settings-card" id="spot" aria-labelledby="spot-title">
              <SectionHeading
                number="01"
                title="スポットを決める"
                description="HUのスタックと参加レンジを定義します。"
                badge="HU · NLHE"
              />
              <span id="spot-title" className="sr-only">スポットを決める</span>
              <div className="field-grid">
                <label className="field">
                  <span className="field-label">
                    実効スタック
                    <span className="field-label-note">1bb = 10 chips</span>
                  </span>
                  <span className="input-wrap">
                    <input
                      className="input has-unit"
                      type="number"
                      min="1.1"
                      step="0.1"
                      value={settings.effectiveStackBb}
                      onChange={(event) => update("effectiveStackBb", numberValue(event.target.value))}
                    />
                    <span className="unit">BB</span>
                  </span>
                  <input
                    className="stack-range"
                    type="range"
                    min="2"
                    max="200"
                    step="1"
                    value={Math.min(200, Math.max(2, settings.effectiveStackBb))}
                    onChange={(event) => update("effectiveStackBb", Number(event.target.value))}
                    aria-label="実効スタックのスライダー"
                  />
                </label>
                <label className="field">
                  <span className="field-label">
                    スモールブラインド
                    <span className="field-label-note">BBは 1.0 固定</span>
                  </span>
                  <span className="input-wrap">
                    <input
                      className="input has-unit"
                      type="number"
                      min="0.1"
                      max="0.9"
                      step="0.1"
                      value={settings.sbBb}
                      onChange={(event) => update("sbBb", numberValue(event.target.value))}
                    />
                    <span className="unit">BB</span>
                  </span>
                  <p className="helper">P0 = SB / Button、P1 = BB として構築します。</p>
                </label>
              </div>

              <div className="seat-row" aria-label="プレイヤー位置">
                <div className="seat">
                  <span className="seat-chip">BTN</span>
                  <span className="seat-copy"><strong>Small blind</strong><small>プリフロップで先にアクション</small></span>
                </div>
                <div className="seat">
                  <span className="seat-chip">BB</span>
                  <span className="seat-copy"><strong>Big blind</strong><small>ポストフロップでは先にアクション</small></span>
                </div>
              </div>

              <div className="range-summary" aria-label="開始レンジ">
                {(["sb", "bb"] as RangeSeat[]).map((seat) => {
                  const range = ranges[seat];
                  const percent = percentOfRange(range);
                  return (
                    <div className="range-player" key={seat}>
                      <span className="range-label">{seat.toUpperCase()}</span>
                      <span className="range-meta">
                        <strong>{range.size} / 169 クラス</strong>
                        <small>{percent.toFixed(1)}% · suit別の重み付けなし</small>
                        <span className="range-bar" aria-hidden="true"><span style={{ width: `${percent}%` }} /></span>
                      </span>
                      <button
                        type="button"
                        className="text-button"
                        onClick={() => {
                          setRangeSeat(seat);
                          setRangeFocus(0);
                          setModal("range");
                        }}
                      >
                        編集
                      </button>
                    </div>
                  );
                })}
              </div>
            </section>

            <section className="settings-card" id="tree" aria-labelledby="tree-title">
              <SectionHeading
                number="02"
                title="ベットツリーを組む"
                description="raise-to サイズと許可する分岐だけを選びます。"
                badge={`${settings.maxRaises} raises max`}
              />
              <span id="tree-title" className="sr-only">ベットツリーを組む</span>
              <div className="field">
                <span className="field-label">
                  オープンサイズ
                  <span className="field-label-note">SB open / BB iso 共通</span>
                </span>
                <div className="size-row">
                  {settings.openSizesBb.map((size) => (
                    <span className="size-chip" key={size}>
                      {size}bb
                      <button
                        type="button"
                        aria-label={`${size}bbを削除`}
                        onClick={() => update("openSizesBb", settings.openSizesBb.filter((item) => item !== size))}
                      >×</button>
                    </span>
                  ))}
                  <span className="add-size">
                    <input
                      className="input"
                      type="number"
                      min="2"
                      step="0.1"
                      value={newOpenSize}
                      onChange={(event) => setNewOpenSize(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") addOpenSize();
                      }}
                      aria-label="追加するオープンサイズ"
                    />
                    <button type="button" className="add-button" onClick={addOpenSize} aria-label="オープンサイズを追加">+</button>
                  </span>
                </div>
                {settings.openSizesBb.length === 0 ? (
                  <p className="inline-callout">通常オープンなし。All-inを有効にすると、SBは fold / jam のみになります。</p>
                ) : null}
              </div>

              {settings.maxRaises > 1 ? (
                <div className="raise-grid">
                  {[
                    { label: "3-bet", note: "前回raise-toへの倍率", fallback: 3 },
                    { label: "4-bet以降", note: "深いレベルでも再利用", fallback: 2.5 },
                  ].map((row, index) => (
                    <label className="raise-row" key={row.label}>
                      <span><strong>{row.label}</strong><small>{row.note}</small></span>
                      <span className="input-wrap">
                        <input
                          className="input has-unit"
                          type="number"
                          min="1.1"
                          step="0.1"
                          value={settings.raiseFactors[index]?.[0] ?? row.fallback}
                          onChange={(event) => setRaiseFactor(index, numberValue(event.target.value))}
                        />
                        <span className="unit">×</span>
                      </span>
                    </label>
                  ))}
                </div>
              ) : null}

              <div className="field-grid" style={{ marginTop: 18 }}>
                <div className="field">
                  <span className="field-label">最大レイズ回数</span>
                  <div className="segment-control" aria-label="最大レイズ回数">
                    {[1, 2, 3, 4, 5].map((value) => (
                      <button
                        type="button"
                        className="segment-button"
                        aria-pressed={settings.maxRaises === value}
                        key={value}
                        onClick={() => setMaximumRaises(value)}
                      >{value}</button>
                    ))}
                  </div>
                </div>
                <div className="toggle-list" style={{ marginTop: 0 }}>
                  <Toggle
                    pressed={settings.allowLimp}
                    onPressedChange={(value) => update("allowLimp", value)}
                    label="Limpを許可"
                    description="SBにcallアクションを追加"
                  />
                  <Toggle
                    pressed={settings.includeAllin}
                    onPressedChange={(value) => update("includeAllin", value)}
                    label="各ノードにJamを追加"
                    description="サイズ指定とは別に明示的なall-inを用意"
                  />
                </div>
              </div>
            </section>

            <section className="settings-card" id="model" aria-labelledby="model-title">
              <SectionHeading
                number="03"
                title="継続モデルを選ぶ"
                description="速度重視か、ポストフロップ近似を含む品質重視か。"
                badge={settings.postflopModel === "equity" ? "FAST" : "EXPERIMENTAL"}
              />
              <span id="model-title" className="sr-only">継続モデルを選ぶ</span>
              <div className="model-grid">
                <button
                  type="button"
                  className="model-card"
                  aria-pressed={settings.postflopModel === "equity"}
                  onClick={() => update("postflopModel", "equity")}
                >
                  <span className="model-icon">EQ</span>
                  <span className="model-details">
                    <strong>Equity showdown</strong>
                    <span>高速 · 接続実行対応</span>
                    <small>All-inは厳密。非All-inはequity realizationで近似します。</small>
                  </span>
                </button>
                <button
                  type="button"
                  className="model-card"
                  aria-pressed={settings.postflopModel === "bucketed"}
                  onClick={() => update("postflopModel", "bucketed")}
                >
                  <span className="model-icon">E²</span>
                  <span className="model-details">
                    <strong>Bucketed blueprint</strong>
                    <span>高精度 · CLI実行</span>
                    <small>EHS²バケットでポストフロップのベット分岐まで近似します。</small>
                  </span>
                </button>
              </div>

              {settings.postflopModel === "equity" ? (
                <div className="reveal-panel">
                  <div className="reveal-title">
                    <strong>Equity realization</strong>
                    <span>非All-in continuationのみ</span>
                  </div>
                  <div className="field-grid">
                    {(["sb", "bb"] as const).map((seat) => (
                      <label className="field" key={seat}>
                        <span className="field-label">{seat.toUpperCase()}</span>
                        <span className="input-wrap">
                          <input
                            className="input has-unit"
                            type="number"
                            min="0.1"
                            step="0.05"
                            value={settings.equityRealization[seat]}
                            onChange={(event) =>
                              update("equityRealization", {
                                ...settings.equityRealization,
                                [seat]: numberValue(event.target.value),
                              })
                            }
                          />
                          <span className="unit">×</span>
                        </span>
                      </label>
                    ))}
                  </div>
                  <p className="inline-callout">100bbではcheckdown近似の影響が大きく、公開品質のレンジにはBucketedを推奨します。</p>
                </div>
              ) : (
                <div className="reveal-panel">
                  <div className="reveal-title">
                    <strong>EHS² resolution</strong>
                    <span>既定値 50 / 20 / 8</span>
                  </div>
                  <div className="bucket-grid">
                    {(["flop", "turn", "river"] as const).map((street) => (
                      <label className="field" key={street}>
                        <span className="field-label">{street.toUpperCase()}</span>
                        <input
                          className="input"
                          type="number"
                          min="1"
                          max="500"
                          step="1"
                          value={settings.buckets[street]}
                          onChange={(event) =>
                            update("buckets", {
                              ...settings.buckets,
                              [street]: Math.trunc(numberValue(event.target.value)),
                            })
                          }
                        />
                      </label>
                    ))}
                  </div>
                  <div className="bucket-grid" style={{ marginTop: 12 }}>
                    {(["flop", "turn", "river"] as const).map((street) => (
                      <label className="field" key={street}>
                        <span className="field-label">{street.toUpperCase()} bet</span>
                        <span className="input-wrap">
                          <input
                            className="input has-unit"
                            type="number"
                            min="0.05"
                            step="0.05"
                            value={settings.postflopBetSizes[street][0] ?? 0}
                            onChange={(event) =>
                              update("postflopBetSizes", {
                                ...settings.postflopBetSizes,
                                [street]: [numberValue(event.target.value)],
                              })
                            }
                          />
                          <span className="unit">POT</span>
                        </span>
                      </label>
                    ))}
                  </div>
                  <p className="inline-callout">初回は抽象化とartifactの準備に約10分。キャッシュ後の再実行は大幅に短縮されます。</p>
                </div>
              )}
            </section>

            <section className="settings-card" id="economics" aria-labelledby="economics-title">
              <SectionHeading
                number="04"
                title="レーキを合わせる"
                description="ゲーム環境のコストをプリフロップ終端へ反映します。"
                badge={settings.rakeMode === "none" ? "NO RAKE" : "RAKED"}
              />
              <span id="economics-title" className="sr-only">レーキを合わせる</span>
              <div className="economics-grid" aria-label="レーキモデル">
                {[
                  { id: "none", label: "No rake", note: "cEV / 検証用" },
                  { id: "percent-cap", label: "Percent + cap", note: "一般的なCash" },
                  { id: "gg-preflop", label: "GG preflop", note: "特定preflop pot" },
                ].map((mode) => (
                  <button
                    type="button"
                    className="economics-button"
                    aria-pressed={settings.rakeMode === mode.id}
                    key={mode.id}
                    onClick={() => update("rakeMode", mode.id as RakeMode)}
                  >
                    <strong>{mode.label}</strong><small>{mode.note}</small>
                  </button>
                ))}
              </div>
              {settings.rakeMode !== "none" ? (
                <div className="reveal-panel">
                  <div className="field-grid">
                    <label className="field">
                      <span className="field-label">レーキ率</span>
                      <span className="input-wrap">
                        <input
                          className="input has-unit"
                          type="number"
                          min="0"
                          max="100"
                          step="0.1"
                          value={Number((settings.rakeRate * 100).toFixed(3))}
                          onChange={(event) => update("rakeRate", numberValue(event.target.value) / 100)}
                        />
                        <span className="unit">%</span>
                      </span>
                    </label>
                    <label className="field">
                      <span className="field-label">上限</span>
                      <span className="input-wrap">
                        <input
                          className="input has-unit"
                          type="number"
                          min="0"
                          step="0.1"
                          value={settings.rakeCap}
                          onChange={(event) => update("rakeCap", numberValue(event.target.value))}
                        />
                        <span className="unit">BB</span>
                      </span>
                    </label>
                  </div>
                  {settings.rakeMode === "percent-cap" ? (
                    <div className="toggle-list">
                      <Toggle
                        pressed={settings.noFlopNoDrop}
                        onPressedChange={(value) => update("noFlopNoDrop", value)}
                        label="No flop, no drop"
                        description="フロップへ進まない終端ではレーキを取らない"
                      />
                    </div>
                  ) : null}
                </div>
              ) : null}
            </section>

            <section className="settings-card" id="run" aria-labelledby="run-title">
              <SectionHeading
                number="05"
                title="精度と実行を整える"
                description="まず既定値で開始し、収束を見て反復数を増やせます。"
                badge={settings.storage.toUpperCase()}
              />
              <span id="run-title" className="sr-only">精度と実行を整える</span>
              <div className="field-grid">
                <label className="field">
                  <span className="field-label">反復回数</span>
                  <input
                    className="input"
                    type="number"
                    min="1"
                    step="100"
                    value={settings.iterations}
                    onChange={(event) => update("iterations", Math.trunc(numberValue(event.target.value)))}
                  />
                  <p className="helper">解析品質を決める主なノブです。</p>
                </label>
                <label className="field">
                  <span className="field-label">収束チェック間隔</span>
                  <input
                    className="input"
                    type="number"
                    min="1"
                    step="25"
                    value={settings.checkEvery}
                    onChange={(event) => update("checkEvery", Math.trunc(numberValue(event.target.value)))}
                  />
                  <p className="helper">小さすぎるとbest-response計算が増えます。</p>
                </label>
              </div>
              <div className="toggle-list">
                <Toggle
                  pressed={settings.cacheEnabled}
                  onPressedChange={(value) => update("cacheEnabled", value)}
                  label="計算済みテーブルをキャッシュ"
                  description="Equity / abstraction / blueprint artifactを再利用"
                />
              </div>
              <details className="advanced">
                <summary>アルゴリズムとストレージ</summary>
                <div className="advanced-body field-grid">
                  <label className="field">
                    <span className="field-label">CFR schedule</span>
                    <select
                      className="select"
                      value={settings.schedule}
                      onChange={(event) => update("schedule", event.target.value as ScheduleKind)}
                    >
                      <option value="dcfr">DCFR（推奨）</option>
                      <option value="cfr-plus">CFR+</option>
                      <option value="vanilla">Vanilla CFR</option>
                      <option value="linear-cfr">Linear CFR</option>
                      <option value="hs-dcfr">HS-DCFR</option>
                    </select>
                  </label>
                  <label className="field">
                    <span className="field-label">Storage</span>
                    <select
                      className="select"
                      value={settings.storage}
                      onChange={(event) => update("storage", event.target.value as StorageKind)}
                    >
                      <option value="f32">f32 · 高速</option>
                      <option value="i16">i16 · 省メモリ</option>
                    </select>
                  </label>
                </div>
              </details>
            </section>
          </div>
        </div>

        <aside className="summary-column" aria-label="設定サマリー">
          <div className="summary-card">
            <div className="summary-head">
              <div className="summary-title-row">
                <h2>Solve preview</h2>
                <button
                  type="button"
                  className="connection-state"
                  onClick={() => setModal("connection")}
                  aria-label="ローカルソルバーの接続設定を開く"
                >
                  <span className={`status-dot ${engineState}`} aria-hidden="true" />
                  {engineState === "online" ? "CONNECTED" : "LOCAL"}
                </button>
              </div>
              <p>{settings.effectiveStackBb}bb · HU NLHE · {settings.postflopModel === "equity" ? "Equity" : "Bucketed"}</p>
            </div>
            <div className="summary-body">
              <div className="metric-grid" aria-live="polite">
                <div className="metric"><span>MEMORY</span><strong>{estimate.memory}</strong></div>
                <div className="metric"><span>TIME</span><strong>{estimate.time}</strong></div>
                <div className="metric"><span>NODES</span><strong>{estimate.nodes}</strong></div>
              </div>

              <div className="summary-section">
                <p className="summary-section-title">ACTION TREE <span>raise-to</span></p>
                <div className="action-tree">
                  <div className="tree-row"><span className="tree-line" /><strong>SB starts</strong><span>{settings.allowLimp ? "limp · fold" : "fold"}</span></div>
                  <div className="tree-row depth-1"><span className="tree-line" /><strong>Open</strong><span>{settings.openSizesBb.length ? settings.openSizesBb.map((size) => `${size}bb`).join(" · ") : "jam only"}</span></div>
                  {settings.maxRaises > 1 ? <div className="tree-row depth-2"><span className="tree-line" /><strong>3-bet</strong><span>{settings.raiseFactors[0]?.[0] ?? 3}×</span></div> : null}
                  {settings.maxRaises > 2 ? <div className="tree-row depth-2"><span className="tree-line" /><strong>4-bet+</strong><span>{settings.raiseFactors[1]?.[0] ?? 2.5}×</span></div> : null}
                  {settings.includeAllin ? <div className="tree-row depth-1"><span className="tree-line" /><strong>Jam</strong><span>each level</span></div> : null}
                </div>
              </div>

              {job?.status === "running" ? (
                <div className="summary-section" aria-live="polite">
                  <p className="summary-section-title">SOLVE PROGRESS <span>{job.progress ? `${job.progress.iteration} iter` : "PREPARING"}</span></p>
                  <div className="range-bar" aria-hidden="true">
                    <span style={{ width: `${Math.min(100, ((job.progress?.iteration ?? 0) / settings.iterations) * 100)}%` }} />
                  </div>
                  <p className="helper">
                    {job.progress
                      ? `NashConv ${job.progress.nashConv.toFixed(5)} · ${job.progress.elapsedSecs.toFixed(1)}s`
                      : "Equityテーブルとゲームツリーを準備しています。"}
                  </p>
                </div>
              ) : null}

              <div className="summary-section">
                <p className="summary-section-title">CHECKS <span>{validationErrors.length ? `${validationErrors.length} ISSUES` : "READY"}</span></p>
                <ul className={`validation-list ${validationErrors.length ? "error" : ""}`}>
                  {validationErrors.length ? (
                    validationErrors.slice(0, 3).map((error) => (
                      <li key={error}><span className="validation-mark">!</span><span>{translateValidation(error)}</span></li>
                    ))
                  ) : (
                    <>
                      <li><span className="validation-mark">✓</span><span>0.1bbチップグリッドに変換可能</span></li>
                      <li><span className="validation-mark">✓</span><span>両プレイヤーのレンジに正の質量あり</span></li>
                      <li><span className="validation-mark">✓</span><span>{estimate.quality}</span></li>
                    </>
                  )}
                </ul>
              </div>

              <div className="summary-actions">
                <button
                  type="button"
                  className="primary-button"
                  disabled={validationErrors.length > 0 || job?.status === "running" || engineBusy}
                  onClick={() => void startSolve()}
                >{primaryLabel}</button>
                <button type="button" className="secondary-button" onClick={() => void copyToml()}>TOMLをコピー</button>
              </div>
              <p className="engine-hint">
                {settings.postflopModel === "bucketed"
                  ? "Bucketedは長時間実行のため、TOMLを書き出してCLIから開始します。"
                  : engineState === "online"
                    ? "認証済みloopback接続。設定はこのPC内だけで処理されます。"
                    : "直接実行するには "}
                {settings.postflopModel === "equity" && engineState !== "online" ? <code>solvers serve</code> : null}
              </p>
            </div>
          </div>
        </aside>
      </div>

      {modal === "connection" ? (
        <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setModal(null);
        }}>
          <section className="modal" role="dialog" aria-modal="true" aria-labelledby="connection-title">
            <header className="modal-head">
              <div><h2 id="connection-title">ローカルソルバーへ接続</h2><p>ローカル開発では <code>solvers serve</code> を起動します。公開URLでは <code>--origin</code> にこのページのOriginを指定してください。</p></div>
              <button type="button" className="icon-button" onClick={() => setModal(null)} aria-label="閉じる">×</button>
            </header>
            <div className="modal-body">
              <div className="field-grid">
                <label className="field full">
                  <span className="field-label">Bridge URL</span>
                  <input className="input" value={bridgeUrl} onChange={(event) => setBridgeUrl(event.target.value)} spellCheck="false" />
                </label>
                <label className="field full">
                  <span className="field-label">One-time token</span>
                  <input className="input" type="password" value={bridgeToken} onChange={(event) => setBridgeToken(event.target.value)} autoComplete="off" spellCheck="false" />
                  <p className="helper">トークンはこのタブのsessionStorageにのみ保存され、ホスト側へ送信されません。</p>
                </label>
              </div>
              <p className="inline-callout">公開URLから接続する場合、ブラウザがローカルネットワークアクセスの許可を求めることがあります。</p>
            </div>
            <footer className="modal-actions">
              <button type="button" className="secondary-button" onClick={() => setModal(null)}>あとで</button>
              <button type="button" className="primary-button" onClick={() => void connectEngine()} disabled={engineState === "checking"}>
                {engineState === "checking" ? "確認中…" : "接続を確認"}
              </button>
            </footer>
          </section>
        </div>
      ) : null}

      {modal === "range" ? (
        <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setModal(null);
        }}>
          <section className="modal range-modal" role="dialog" aria-modal="true" aria-labelledby="range-title">
            <header className="modal-head">
              <div><h2 id="range-title">開始レンジを編集</h2><p>169クラス単位。矢印キーで移動し、Spaceで選択できます。</p></div>
              <button type="button" className="icon-button" onClick={() => setModal(null)} aria-label="閉じる">×</button>
            </header>
            <div className="modal-body">
              <div className="range-toolbar">
                <div className="range-tabs" aria-label="プレイヤー">
                  {(["sb", "bb"] as RangeSeat[]).map((seat) => (
                    <button type="button" className="range-tab" aria-pressed={rangeSeat === seat} key={seat} onClick={() => {
                      setRangeSeat(seat);
                      setRangeFocus(0);
                    }}>{seat.toUpperCase()}</button>
                  ))}
                </div>
                <div className="range-tools">
                  <button type="button" className="range-tool" onClick={() => setRange(new Set(HANDS))}>すべて</button>
                  <button type="button" className="range-tool" onClick={() => setRange(new Set())}>クリア</button>
                </div>
              </div>
              <div className="range-grid-wrap">
                <div className="range-grid" role="grid" aria-label={`${rangeSeat.toUpperCase()}の169ハンドクラス`}>
                  {HANDS.map((hand, index) => (
                    <button
                      id={`range-${index}`}
                      type="button"
                      role="gridcell"
                      className={`range-cell ${hand.length === 2 ? "pair" : hand.endsWith("s") ? "suited" : "offsuit"}`}
                      aria-selected={ranges[rangeSeat].has(hand)}
                      aria-label={`${rangeSeat.toUpperCase()} ${hand} ${ranges[rangeSeat].has(hand) ? "選択中" : "未選択"}`}
                      tabIndex={rangeFocus === index ? 0 : -1}
                      key={hand}
                      onFocus={() => setRangeFocus(index)}
                      onKeyDown={(event) => onRangeKeyDown(event, index)}
                      onClick={() => toggleHand(hand)}
                    >{hand}</button>
                  ))}
                </div>
              </div>
              <div className="range-summary-line">
                <span>Pairはコーラル、Suited/Offsuitはセージで選択表示</span>
                <strong>{ranges[rangeSeat].size} classes · {percentOfRange(ranges[rangeSeat]).toFixed(1)}%</strong>
              </div>
            </div>
            <footer className="modal-actions">
              <button type="button" className="secondary-button" onClick={() => setModal(null)}>キャンセル</button>
              <button type="button" className="primary-button" onClick={saveRanges}>レンジを適用</button>
            </footer>
          </section>
        </div>
      ) : null}

      {modal === "review" ? (
        <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setModal(null);
        }}>
          <section className="modal" role="dialog" aria-modal="true" aria-labelledby="review-title">
            <header className="modal-head">
              <div><h2 id="review-title">生成されたSolveConfig</h2><p>現在のPFソルバーが受け付けるTOML形式です。</p></div>
              <button type="button" className="icon-button" onClick={() => setModal(null)} aria-label="閉じる">×</button>
            </header>
            <div className="modal-body"><pre className="code-preview">{toml}</pre></div>
            <footer className="modal-actions">
              <button type="button" className="secondary-button" onClick={() => void copyToml()}>コピー</button>
              <button type="button" className="primary-button" onClick={() => downloadText("preflop.toml", toml, "text/plain;charset=utf-8")}>TOMLを保存</button>
            </footer>
          </section>
        </div>
      ) : null}

      {modal === "result" && resultJson ? (
        <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setModal(null);
        }}>
          <section className="modal" role="dialog" aria-modal="true" aria-labelledby="result-title">
            <header className="modal-head">
              <div><h2 id="result-title">PF解析が完了しました</h2><p>169クラスのroot戦略を含むsolver resultです。</p></div>
              <button type="button" className="icon-button" onClick={() => setModal(null)} aria-label="閉じる">×</button>
            </header>
            <div className="modal-body"><pre className="code-preview">{resultJson}</pre></div>
            <footer className="modal-actions">
              <button type="button" className="secondary-button" onClick={() => void navigator.clipboard.writeText(resultJson)}>コピー</button>
              <button type="button" className="primary-button" onClick={() => downloadText("preflop-result.json", resultJson, "application/json")}>JSONを保存</button>
            </footer>
          </section>
        </div>
      ) : null}

      {toast ? <div className="toast" role="status" aria-live="polite">{toast}</div> : null}
    </main>
  );
}
