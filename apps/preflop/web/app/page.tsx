"use client";

import {
  type AriaAttributes,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  DEFAULT_SETTINGS,
  PRESETS,
  createBucketProfiles,
  createMultiwaySeats,
  parseNumberPaste,
  positionsForTableSize,
  estimateSolve,
  generateToml,
  validateSettings,
  type MultiwayStreetBetting,
  type PreflopSettings,
  type RakeMode,
  type ScheduleKind,
  type StorageKind,
  type StreetValues,
} from "./preflop-config";
import {
  type MultiwayResultV2,
  type MultiwayStrategyBlock,
  type StrategyPage,
} from "./multiway-result";
import CommaListInput from "./CommaListInput";
import MultiwayResultExplorer from "./MultiwayResultExplorer";
import MultiwaySeatOverrides from "./MultiwaySeatOverrides";
import MultiwaySeatTabs from "./MultiwaySeatTabs";

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
const VALIDATION_SUMMARY_ID = "validation-summary";
const VALIDATION_MESSAGES_ID = "validation-messages";

type RangeSeat = "sb" | "bb";
type EngineState = "offline" | "checking" | "online";
type BridgeApiVersion = 1 | 2;
type ModalKind = "connection" | "range" | "review" | "result" | null;

interface BridgeProgress {
  iteration?: number;
  elapsedSecs?: number;
  explP0?: number;
  explP1?: number;
  nashConv?: number;
  phase?: string;
  sweeps?: number;
  totalSweeps?: number;
  infosets?: number;
  memoryBytes?: number;
  seats?: Array<{
    seat: number;
    profileEv?: {
      mean: number;
      stderr: number;
      ci95: [number, number];
    } | null;
    averagePositiveRegret: number;
    strategyDriftL1: number;
  }>;
}

interface BridgeJob {
  id: string;
  status:
    | "running"
    | "cancelling"
    | "cancelled"
    | "succeeded"
    | "failed"
    | "resource_limit";
  progress?: BridgeProgress | null;
  apiVersion?: BridgeApiVersion;
  resultUrl?: string | null;
  checkpointUrl?: string | null;
  error?: { code: string; message: string } | null;
}

interface SubmittedJobSnapshot {
  mode: PreflopSettings["mode"];
  progressTarget: number;
  positions: string[];
}

interface BridgeHealth {
  service: string;
  version: string;
  apiVersion: number;
  busy: boolean;
  capabilities?: {
    maxPlayers: number;
    maxIcmField: number;
    exactIcmField: number;
    configSchemas: string[];
    resultSchemas: number[];
    stages: string[];
  };
}

const STEPS = [
  { id: "spot", number: "01", title: "スポット", note: "Stack & ranges" },
  { id: "tree", number: "02", title: "ベットツリー", note: "Actions & sizes" },
  { id: "model", number: "03", title: "継続モデル", note: "Postflop model" },
  { id: "economics", number: "04", title: "レーキ", note: "Economics" },
  { id: "run", number: "05", title: "実行精度", note: "Accuracy & run" },
];

function cloneMultiwayPostflop(
  postflop: StreetValues<MultiwayStreetBetting>,
): StreetValues<MultiwayStreetBetting> {
  return Object.fromEntries(
    (["flop", "turn", "river"] as const).map((street) => [
      street,
      {
        ...postflop[street],
        betSizes: [...postflop[street].betSizes],
        raiseSizes: [...postflop[street].raiseSizes],
      },
    ]),
  ) as unknown as StreetValues<MultiwayStreetBetting>;
}

function cloneSettings(settings: PreflopSettings): PreflopSettings {
  return {
    ...settings,
    openSizesBb: [...settings.openSizesBb],
    isolateSizesBb: [...settings.isolateSizesBb],
    raiseFactors: settings.raiseFactors.map((level) => [...level]),
    equityRealization: { ...settings.equityRealization },
    buckets: { ...settings.buckets },
    seats: settings.seats.map((seat) => ({
      ...seat,
      betting: seat.betting
        ? {
            openSizesBb: [...seat.betting.openSizesBb],
            isolateSizesBb: [...seat.betting.isolateSizesBb],
            raiseFactors: [...seat.betting.raiseFactors],
            postflop: cloneMultiwayPostflop(seat.betting.postflop),
          }
        : undefined,
    })),
    bucketProfiles: settings.bucketProfiles.map((profile) => ({ ...profile })),
    postflopBetSizes: {
      flop: [...settings.postflopBetSizes.flop],
      turn: [...settings.postflopBetSizes.turn],
      river: [...settings.postflopBetSizes.river],
    },
    multiwayPostflopBetting: cloneMultiwayPostflop(settings.multiwayPostflopBetting),
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
  if (error.includes("Small blind")) {
    return error.includes("0.001")
      ? "SB は 0.001〜0.999bb の範囲で指定してください。"
      : "SB は 0.1〜0.9bb の範囲で指定してください。";
  }
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

type ValidationProps = Pick<
  AriaAttributes,
  "aria-invalid" | "aria-errormessage"
>;

function validationFieldsForError(
  error: string,
  settings: PreflopSettings,
): string[] {
  const lower = error.toLowerCase();

  for (const [index, seat] of settings.seats.entries()) {
    const label = (seat.position || `Seat #${index + 1}`).toLowerCase();
    if (!lower.startsWith(`${label} `)) continue;
    if (!lower.includes("override")) {
      if (lower.includes(" stack")) return [`seat-${index}-stack`];
      if (lower.includes(" range")) return [`seat-${index}-range`];
    }
    if (lower.includes("override")) {
      for (const street of ["flop", "turn", "river"] as const) {
        if (!lower.includes(` ${street} `)) continue;
        if (lower.includes(" bet size")) {
          return [`seat-${index}-override-${street}-betSizes`];
        }
        if (lower.includes(" raise size")) {
          return [`seat-${index}-override-${street}-raiseSizes`];
        }
        if (lower.includes("aggressive-action cap")) {
          return [`seat-${index}-override-${street}-cap`];
        }
      }
      if (lower.includes(" open size")) {
        return [`seat-${index}-override-openSizesBb`];
      }
      if (lower.includes(" isolate size")) {
        return [`seat-${index}-override-isolateSizesBb`];
      }
      if (lower.includes(" raise factor")) {
        return [`seat-${index}-override-raiseFactors`];
      }
    }
  }

  const bucketMatch = error.match(/^(\d+)-player (preflop|flop|turn|river) buckets/i);
  if (bucketMatch) {
    const profileIndex = settings.bucketProfiles.findIndex(
      (profile) => profile.activePlayers === Number(bucketMatch[1]),
    );
    if (profileIndex >= 0) return [`bucket-${profileIndex}-${bucketMatch[2].toLowerCase()}`];
  }
  for (const street of ["flop", "turn", "river"] as const) {
    if (lower.startsWith(`${street} bet size`)) return [`street-${street}-betSizes`];
    if (lower.startsWith(`${street} raise size`)) return [`street-${street}-raiseSizes`];
    if (lower.startsWith(`${street} aggressive-action cap`)) return [`street-${street}-cap`];
  }

  if (lower.includes("table size") || lower.includes("seat count") || lower.includes("bucket profiles")) return ["table-size"];
  if (lower.includes("small blind")) return ["small-blind"];
  if (lower.startsWith("ante") || lower.includes("ante mode")) return ["ante"];
  if (lower.startsWith("open size")) return ["preflop-open"];
  if (lower.startsWith("isolate size")) return ["preflop-isolate"];
  if (lower.startsWith("raise level") || lower.startsWith("raise factors")) return ["preflop-raise"];
  if (lower.startsWith("maximum raises")) return ["preflop-cap"];
  if (lower.includes("external-sampling sweeps")) return ["run-sweeps"];
  if (lower.includes("external-sampling seed")) return ["run-seed"];
  if (lower.includes("checkpoint cadence")) return ["run-checkpoint"];
  if (lower.includes("evaluation cadence")) return ["run-evaluation-cadence"];
  if (lower.includes("evaluation samples")) return ["run-evaluation-samples"];
  if (lower.includes("memory limit")) return ["run-memory"];
  if (lower.includes("resume checkpoint")) return ["run-resume"];
  if (lower.includes("outside stack")) return ["icm-outside"];
  if (lower.includes("icm seed")) return ["icm-seed"];
  if (lower.includes("sampled icm")) return ["icm-samples"];
  if (lower.includes("payout") || lower.includes("tournament icm")) return ["icm-payouts"];
  return [];
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
  const [bulkSeatIndex, setBulkSeatIndex] = useState(0);
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
  const [submittedJob, setSubmittedJob] = useState<SubmittedJobSnapshot | null>(null);
  const [resultJson, setResultJson] = useState("");
  const [multiwayResult, setMultiwayResult] = useState<MultiwayResultV2 | null>(null);
  const [strategyBlocks, setStrategyBlocks] = useState<MultiwayStrategyBlock[]>([]);
  const [strategyCursor, setStrategyCursor] = useState<string | null>(null);
  const [strategyBusy, setStrategyBusy] = useState(false);
  const [bridgeApiVersion, setBridgeApiVersion] =
    useState<BridgeApiVersion>(1);
  const [bridgeCapabilities, setBridgeCapabilities] =
    useState<BridgeHealth["capabilities"]>();
  const modalRef = useRef<HTMLElement | null>(null);
  const lastFocusedRef = useRef<HTMLElement | null>(null);


  const validationErrors = useMemo(() => validateSettings(settings), [settings]);
  const validationByField = useMemo(() => {
    const byField = new Map<string, string[]>();
    for (const error of validationErrors) {
      for (const field of validationFieldsForError(error, settings)) {
        byField.set(field, [...(byField.get(field) ?? []), error]);
      }
    }
    return byField;
  }, [settings, validationErrors]);
  const fieldValidationProps = useCallback(
    (field: string): ValidationProps =>
      validationByField.has(field)
        ? {
            "aria-invalid": true,
            "aria-errormessage": `validation-${field}`,
          }
        : {},
    [validationByField],
  );
  const estimate = useMemo(() => estimateSolve(settings), [settings]);
  const toml = useMemo(() => generateToml(settings), [settings]);
  const parsedOutsideStacks = useMemo(
    () => parseNumberPaste(settings.outsideStacksText),
    [settings.outsideStacksText],
  );
  const parsedPayouts = useMemo(
    () => parseNumberPaste(settings.payoutsText),
    [settings.payoutsText],
  );
  const outsidePlayerCount = parsedOutsideStacks.length;
  const payoutCount = parsedPayouts.length;
  const icmFieldSize = settings.tableSize + outsidePlayerCount;
  const payoutReadback = useMemo(
    () =>
      Array.from(
        { length: Math.max(icmFieldSize, parsedPayouts.length) },
        (_, index) => ({
          place: index + 1,
          amount: parsedPayouts[index] ?? 0,
          padded: index >= parsedPayouts.length,
        }),
      ),
    [icmFieldSize, parsedPayouts],
  );
  const jobMode = submittedJob?.mode ?? settings.mode;
  const jobPositions =
    submittedJob?.positions ?? settings.seats.map((seat) => seat.position);
  const progressCurrent =
    jobMode === "multiway"
      ? (job?.progress?.sweeps ?? 0)
      : (job?.progress?.iteration ?? 0);
  const progressTarget =
    submittedJob?.progressTarget ??
    (settings.mode === "multiway"
      ? settings.externalSamplingSweeps
      : settings.iterations);
  const progressPercent =
    progressTarget > 0
      ? Math.min(100, (progressCurrent / progressTarget) * 100)
      : 0;



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
      let response = await authenticatedFetch("/v2/health");
      let version: BridgeApiVersion = 2;
      if (!response.ok) {
        response = await authenticatedFetch("/v1/health");
        version = 1;
      }
      const health = await responseJson<BridgeHealth>(response);
      if (health.service !== "solvers" || ![1, 2].includes(health.apiVersion)) {
        throw new Error("互換性のないローカルサービスです。");
      }
      setEngineState("online");
      setEngineBusy(health.busy);
      setEngineVersion(health.version);
      setBridgeApiVersion(version);
      setBridgeCapabilities(health.capabilities);
      sessionStorage.setItem("solvers.bridgeUrl", bridgeUrl);
      sessionStorage.setItem("solvers.bridgeToken", bridgeToken);
      setModal(null);
      showToast(
        version === 2
          ? "v2 Multiway対応ソルバーへ接続しました。"
          : "HU互換モードでソルバーへ接続しました。",
      );
    } catch (error) {
      setEngineState("offline");
      setBridgeCapabilities(undefined);
      showToast(error instanceof Error ? error.message : "接続できませんでした。");
    }
  }, [authenticatedFetch, bridgeToken, bridgeUrl, showToast]);

  const loadStrategyPage = useCallback(
    async (jobId: string, cursor?: string) => {
      setStrategyBusy(true);
      try {
        const query = new URLSearchParams({ limit: "100" });
        if (cursor) query.set("cursor", cursor);
        const response = await authenticatedFetch(
          `/v2/jobs/${encodeURIComponent(jobId)}/strategies?${query}`,
        );
        const page = await responseJson<StrategyPage>(response);
        setStrategyBlocks((current) =>
          cursor ? [...current, ...page.items] : page.items,
        );
        setStrategyCursor(page.nextCursor ?? null);
      } catch (error) {
        showToast(error instanceof Error ? error.message : "戦略ブロックを取得できませんでした。");
      } finally {
        setStrategyBusy(false);
      }
    },
    [authenticatedFetch, showToast],
  );

  const fetchResult = useCallback(
    async (resultUrl: string, jobId?: string) => {
      const path = resultUrl.startsWith("http")
        ? resultUrl.replace(bridgeUrl.replace(/\/$/, ""), "")
        : resultUrl;
      const response = await authenticatedFetch(path);
      if (!response.ok) throw new Error("解析結果を取得できませんでした。");
      const text = await response.text();
      const parsed: unknown = JSON.parse(text);
      setResultJson(JSON.stringify(parsed, null, 2));
      const candidate = parsed as Partial<MultiwayResultV2>;
      if (
        candidate.kind === "preflop-multiway" &&
        candidate.schemaVersion === 2
      ) {
        setMultiwayResult(candidate as MultiwayResultV2);
        setStrategyBlocks([]);
        setStrategyCursor(null);
        if (jobId) await loadStrategyPage(jobId);
      } else {
        setMultiwayResult(null);
        setStrategyBlocks([]);
        setStrategyCursor(null);
      }
      setModal("result");
    },
    [authenticatedFetch, bridgeUrl, loadStrategyPage],
  );

  const refreshJob = useCallback(async () => {
    if (!job || (job.status !== "running" && job.status !== "cancelling")) return;
    try {
      const apiVersion = job.apiVersion ?? bridgeApiVersion;
      const response = await authenticatedFetch(
        `/v${apiVersion}/jobs/${encodeURIComponent(job.id)}`,
      );
      const nextJob = await responseJson<BridgeJob>(response);
      setJob({ ...nextJob, apiVersion });
      const hasTerminalResult =
        nextJob.status === "succeeded" ||
        nextJob.status === "cancelled" ||
        nextJob.status === "resource_limit";
      if (hasTerminalResult) {
        setEngineBusy(false);
        showToast(nextJob.status === "succeeded" ? "解析が完了しました。" : nextJob.status === "cancelled" ? "解析をキャンセルしました。" : "リソース上限で解析を停止しました。");
        if (nextJob.resultUrl) {
          await fetchResult(nextJob.resultUrl, job.id);
        }
      } else if (nextJob.status === "failed") {
        setEngineBusy(false);
        showToast(nextJob.error?.message ?? "解析に失敗しました。");
      }
    } catch {
      setEngineState("offline");
    }
  }, [authenticatedFetch, bridgeApiVersion, fetchResult, job, showToast]);

  const cancelSolve = useCallback(
    async () => {
      const apiVersion = job?.apiVersion ?? bridgeApiVersion;
      if (
        !job || apiVersion !== 2 ||
        (job.status !== "running" && job.status !== "cancelling")
      ) return;
      try {
        const response = await authenticatedFetch(
          `/v2/jobs/${encodeURIComponent(job.id)}/cancel`,
          { method: "POST" },
        );
        const nextJob = await responseJson<BridgeJob>(response);
        showToast(
          nextJob.status === "cancelled" ? "解析をキャンセルしました。" : "キャンセルを要求しました。",
        );
        if (
          nextJob.status === "cancelled" ||
          nextJob.status === "resource_limit" ||
          nextJob.status === "succeeded"
        ) {
          const statusResponse = await authenticatedFetch(
            `/v2/jobs/${encodeURIComponent(job.id)}`,
          );
          const terminalJob = await responseJson<BridgeJob>(statusResponse);
          setJob({ ...terminalJob, apiVersion: 2 });
          setEngineBusy(false);
          if (terminalJob.resultUrl) {
            await fetchResult(terminalJob.resultUrl, job.id);
          }
        } else {
          setJob({ ...job, ...nextJob, apiVersion: 2 });
        }
      } catch (error) {
        showToast(error instanceof Error ? error.message : "キャンセルできませんでした。");
      }
    },
    [authenticatedFetch, bridgeApiVersion, fetchResult, job, showToast],
  );

  useEffect(() => {
    if (!job || (job.status !== "running" && job.status !== "cancelling")) return;
    const timer = window.setInterval(() => void refreshJob(), 1000);
    return () => window.clearInterval(timer);
  }, [job, refreshJob]);

  useEffect(() => {
    if (!modal) return;
    lastFocusedRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    window.requestAnimationFrame(() => {
      modalRef.current
        ?.querySelector<HTMLElement>(
          "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
        )
        ?.focus();
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setModal(null);
      if (event.key !== "Tab" || !modalRef.current) return;
      const focusable = Array.from(
        modalRef.current.querySelectorAll<HTMLElement>(
          "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
        ),
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      lastFocusedRef.current?.focus();
    };
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
  function setWorkbenchMode(mode: PreflopSettings["mode"]) {
    const preset = PRESETS.find((item) => item.settings.mode === mode);
    if (preset) applyPreset(preset.id);
  }

  function setMultiwayTableSize(tableSize: number) {
    setSettings((current) => {
      const byPosition = new Map(
        current.seats.map((seat) => [seat.position, seat]),
      );
      const positions = positionsForTableSize(tableSize);
      const defaultStack = current.seats[0]?.stackBb ?? 30;
      const seats = createMultiwaySeats(tableSize, defaultStack).map(
        (seat, index) => ({
          ...seat,
          position: positions[index],
          stackBb: byPosition.get(positions[index])?.stackBb ?? seat.stackBb,
          range: byPosition.get(positions[index])?.range ?? "",
          betting: byPosition.get(positions[index])?.betting,
        }),
      );
      const byPlayers = new Map(
        current.bucketProfiles.map((profile) => [
          profile.activePlayers,
          profile,
        ]),
      );
      const bucketProfiles = createBucketProfiles(tableSize, 64).map(
        (profile) => ({
          ...profile,
          ...byPlayers.get(profile.activePlayers),
        }),
      );
      return { ...current, tableSize, seats, bucketProfiles };
    });
    setActivePreset("custom");
  }

  function updateMultiwaySeat(
    index: number,
    key: "stackBb" | "range",
    value: number | string,
  ) {
    update(
      "seats",
      settings.seats.map((seat, seatIndex) =>
        seatIndex === index ? { ...seat, [key]: value } : seat,
      ),
    );
  }

  function toggleSeatBetting(index: number) {
    update(
      "seats",
      settings.seats.map((seat, seatIndex) => {
        if (seatIndex !== index) return seat;
        if (seat.betting) return { ...seat, betting: undefined };
        return {
          ...seat,
          betting: {
            openSizesBb: [...settings.openSizesBb],
            isolateSizesBb: [...settings.isolateSizesBb],
            raiseFactors: [...settings.raiseFactors.flat()],
            postflop: cloneMultiwayPostflop(settings.multiwayPostflopBetting),
          },
        };
      }),
    );
  }

  function updateSeatPreflopSizes(
    index: number,
    key: "openSizesBb" | "isolateSizesBb" | "raiseFactors",
    values: number[],
  ) {
    update(
      "seats",
      settings.seats.map((seat, seatIndex) => {
        if (seatIndex !== index || !seat.betting) return seat;
        return { ...seat, betting: { ...seat.betting, [key]: values } };
      }),
    );
  }

  function updateSeatPostflopSizes(
    index: number,
    street: "flop" | "turn" | "river",
    key: "betSizes" | "raiseSizes",
    values: number[],
  ) {
    update(
      "seats",
      settings.seats.map((seat, seatIndex) => {
        if (seatIndex !== index || !seat.betting) return seat;
        return {
          ...seat,
          betting: {
            ...seat.betting,
            postflop: {
              ...seat.betting.postflop,
              [street]: {
                ...seat.betting.postflop[street],
                [key]: values,
              },
            },
          },
        };
      }),
    );
  }

  function updateSeatPostflopOption(
    index: number,
    street: "flop" | "turn" | "river",
    key: "maxAggressiveActions" | "includeAllin",
    value: number | boolean,
  ) {
    update(
      "seats",
      settings.seats.map((seat, seatIndex) => {
        if (seatIndex !== index || !seat.betting) return seat;
        return {
          ...seat,
          betting: {
            ...seat.betting,
            postflop: {
              ...seat.betting.postflop,
              [street]: { ...seat.betting.postflop[street], [key]: value },
            },
          },
        };
      }),
    );
  }

  function applyMultiwaySeatToAll() {
    setSettings((current) => {
      const source = current.seats[Math.min(bulkSeatIndex, current.seats.length - 1)];
      if (!source) return current;
      return {
        ...current,
        seats: current.seats.map((seat) => ({
          ...seat,
          stackBb: source.stackBb,
          range: source.range,
          betting: source.betting
            ? {
                openSizesBb: [...source.betting.openSizesBb],
                isolateSizesBb: [...source.betting.isolateSizesBb],
                raiseFactors: [...source.betting.raiseFactors],
                postflop: cloneMultiwayPostflop(source.betting.postflop),
              }
            : undefined,
        })),
      };
    });
    setActivePreset("custom");
    showToast("選択したseatのstack・range・sizingを全席へ適用しました。");
  }

  function updateBucketProfile(
    index: number,
    street: "preflop" | "flop" | "turn" | "river",
    value: number,
  ) {
    update(
      "bucketProfiles",
      settings.bucketProfiles.map((profile, profileIndex) =>
        profileIndex === index
          ? { ...profile, [street]: Math.trunc(value) }
          : profile,
      ),
    );
  }

  function setUtilityMode(mode: PreflopSettings["utilityMode"]) {
    setSettings((current) => ({
      ...current,
      utilityMode: mode,
      icmMethod: "auto",
      rakeMode: mode === "icm" ? "none" : current.rakeMode,
    }));
    setActivePreset("custom");
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
    if (settings.mode === "hu" && settings.postflopModel === "bucketed") {
      setModal("review");
      showToast("BucketedはTOMLを書き出し、CLIから実行できます。");
      return;
    }
    if (engineState !== "online") {
      setModal("connection");
      return;
    }
    if (settings.mode === "multiway" && bridgeApiVersion < 2) {
      setModal("connection");
      showToast("Multiway実行にはv2対応のローカルソルバーが必要です。");
      return;
    }
    const submittedSnapshot: SubmittedJobSnapshot = {
      mode: settings.mode,
      progressTarget:
        settings.mode === "multiway"
          ? settings.externalSamplingSweeps
          : settings.iterations,
      positions: settings.mode === "multiway" ? settings.seats.map((seat) => seat.position) : ["SB", "BB"],
    };
    try {
      setEngineBusy(true);
      const apiVersion: BridgeApiVersion =
        submittedSnapshot.mode === "multiway" ? 2 : 1;
      if (apiVersion === 2) {
        await responseJson<{ valid: boolean; schemaVersion: number }>(
          await authenticatedFetch("/v2/validate", {
            method: "POST",
            body: JSON.stringify({ configToml: toml }),
          }),
        );
      }
      const createPayload =
        apiVersion === 2 && settings.resumeCheckpoint.trim()
          ? {
              configToml: toml,
              resumeCheckpointUrl: settings.resumeCheckpoint.trim(),
            }
          : { configToml: toml };
      const jobRequest: RequestInit = {
        method: "POST",
        body: JSON.stringify(createPayload),
      };
      const response = await (
        apiVersion === 2
          ? authenticatedFetch("/v2/jobs", jobRequest)
          : authenticatedFetch("/v1/jobs", jobRequest)
      );
      const nextJob = await responseJson<BridgeJob>(response);
      setJob({ ...nextJob, apiVersion });
      setSubmittedJob(submittedSnapshot);
      showToast(
        submittedSnapshot.mode === "multiway"
          ? "Multiway外部サンプリングを開始しました。"
          : "PF解析を開始しました。",
      );
    } catch (error) {
      setEngineBusy(false);
      showToast(error instanceof Error ? error.message : "解析を開始できませんでした。");
    }
  }

  const primaryLabel =
    job?.status === "running" || job?.status === "cancelling"
      ? "解析中…"
      : settings.mode === "multiway" && settings.resumeCheckpoint.trim()
        ? "Checkpointから解析を再開"
        : settings.mode === "hu" && settings.postflopModel === "bucketed"
        ? "設定を確認"
        : engineState === "online"
          ? settings.mode === "multiway" && bridgeApiVersion < 2
            ? "v2ソルバーへ接続"
            : "この設定で解析を開始"
          : "ソルバーへ接続";

  return (
    <main className="app-shell">
      <div className="sr-only">
        {Array.from(validationByField.entries()).map(([field, errors]) => (
          <span id={`validation-${field}`} key={field}>
            {errors.map(translateValidation).join(" ")}
          </span>
        ))}
      </div>
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
                ? settings.mode === "multiway"
                  ? `${settings.tableSize}-max Multiway カスタム`
                  : `${settings.effectiveStackBb}bb HU カスタム`
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
                {settings.mode === "multiway"
                  ? "Multiwayプリフロップを、"
                  : "HUプリフロップを、"}
                <span>迷わず設計。</span>
              </h1>
            </div>
            <p className="lead">
              {settings.mode === "multiway"
                ? "3〜9席のスタック、レンジ、ICM、全ストリート抽象化を一つの流れで設計。v2 bridgeへそのまま渡せます。"
                : "スポット、アクション、継続モデルを一つの流れで設定。入力中もツリー規模と計算負荷を確認できます。"}
            </p>
            <div
              className="workbench-mode-switch"
              role="group"
              aria-label="テーブルモード"
            >
              <button
                type="button"
                aria-pressed={settings.mode === "hu"}
                onClick={() => setWorkbenchMode("hu")}
              >
                <strong>Heads-up</strong>
                <small>既存の高速・bucketed PF</small>
              </button>
              <button
                type="button"
                aria-pressed={settings.mode === "multiway"}
                onClick={() => setWorkbenchMode("multiway")}
              >
                <strong>Multiway</strong>
                <small>3–9 seats · cEV / ICM</small>
              </button>
            </div>
            {settings.mode === "multiway" ? (
              <div className="approximation-notice" role="note">
                <strong>Approximate multiway profile</strong>
                <p>External-samplingの近似profileです。Nash / GTO収束や最良応答の保証はありません。</p>
              </div>
            ) : null}
            <div className="preset-row" aria-label="プリフロッププリセット">
              {PRESETS.filter(
                (preset) => preset.settings.mode === settings.mode,
              ).map((preset) => (
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
                description={
                  settings.mode === "multiway"
                    ? "3〜9席の標準ポジション、個別スタック、参加レンジを定義します。"
                    : "HUのスタックと参加レンジを定義します。"
                }
                badge={
                  settings.mode === "multiway" ? `${settings.tableSize}-MAX · NLHE` : "HU · NLHE"
                }
              />
              <span id="spot-title" className="sr-only">スポットを決める</span>
              {settings.mode === "hu" ? (
                <>
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
                </>
              ) : (
                <div className="multiway-spot">
                  <div className="field-grid three">
                    <label className="field">
                      <span className="field-label">テーブル人数</span>
                      <select
                        className="select"
                        value={settings.tableSize}
                        {...fieldValidationProps("table-size")}
                        onChange={(event) =>
                          setMultiwayTableSize(Number(event.target.value))
                        }
                      >
                        {Array.from({ length: 7 }, (_, index) => index + 3).map(
                          (size) => (
                            <option key={size} value={size}>
                              {size}-max
                            </option>
                          ),
                        )}
                      </select>
                    </label>
                    <label className="field">
                      <span className="field-label">
                        スモールブラインド
                        <span className="field-label-note">BB = 1.0</span>
                      </span>
                      <span className="input-wrap">
                        <input
                          className="input has-unit"
                          type="number"
                          min="0.001"
                          max="0.999"
                          step="0.001"
                          {...fieldValidationProps("small-blind")}
                          value={settings.sbBb}
                          onChange={(event) =>
                            update("sbBb", numberValue(event.target.value))
                          }
                        />
                        <span className="unit">BB</span>
                      </span>
                    </label>
                    <label className="field">
                      <span className="field-label">Ante方式</span>
                      <select
                        className="select"
                        value={settings.anteMode}
                        {...fieldValidationProps("ante")}
                        onChange={(event) =>
                          update(
                            "anteMode",
                            event.target.value as PreflopSettings["anteMode"],
                          )
                        }
                      >
                        <option value="none">No ante</option>
                        <option value="ante">Each player ante</option>
                        <option value="big-blind-ante">Big blind ante</option>
                      </select>
                    </label>
                  </div>
                  {settings.anteMode !== "none" ? (
                    <label className="field compact-field">
                      <span className="field-label">Ante量</span>
                      <span className="input-wrap">
                        <input
                          className="input has-unit"
                          type="number"
                          min="0"
                          max="1000"
                          step="0.025"
                          {...fieldValidationProps("ante")}
                          value={settings.anteBb}
                          onChange={(event) =>
                            update("anteBb", numberValue(event.target.value))
                          }
                        />
                        <span className="unit">BB</span>
                      </span>
                    </label>
                  ) : null}
                  <MultiwaySeatTabs seats={settings.seats} />
                  <div className="table-scroll">
                    <table className="seat-table">
                      <caption className="sr-only">
                        標準ポジションごとのスタックと開始レンジ
                      </caption>
                      <thead>
                        <tr>
                          <th scope="col">Position</th>
                          <th scope="col">Stack</th>
                          <th scope="col">Starting range</th>
                          <th scope="col">Sizing</th>
                        </tr>
                      </thead>
                      <tbody>
                        {settings.seats.map((seat, index) => (
                          <tr id={`multiway-seat-row-${index}`} key={seat.id} aria-labelledby={`multiway-seat-tab-${index}`}>
                            <th scope="row">
                              <span className="seat-chip">{seat.position}</span>
                            </th>
                            <td>
                              <label>
                                <span className="sr-only">
                                  {seat.position} stack in big blinds
                                </span>
                                <span className="input-wrap">
                                  <input
                                    className="input has-unit"
                                    type="number"
                                    min="0.1"
                                    max="1000"
                                    step="0.1"
                                    value={seat.stackBb}
                                    {...fieldValidationProps(`seat-${index}-stack`)}
                                    onChange={(event) =>
                                      updateMultiwaySeat(
                                        index,
                                        "stackBb",
                                        numberValue(event.target.value),
                                      )
                                    }
                                  />
                                  <span className="unit">BB</span>
                                </span>
                              </label>
                            </td>
                            <td>
                              <label>
                                <span className="sr-only">
                                  {seat.position} starting range
                                </span>
                                <input
                                  className="input range-input"
                                  value={seat.range}
                                  {...fieldValidationProps(`seat-${index}-range`)}
                                  placeholder="空欄 = full range"
                                  spellCheck="false"
                                  onChange={(event) =>
                                    updateMultiwaySeat(
                                      index,
                                      "range",
                                      event.target.value,
                                    )
                                  }
                                />
                              </label>
                            </td>
                            <td>
                              <button
                                type="button"
                                className="text-button"
                                aria-pressed={Boolean(seat.betting)}
                                onClick={() => toggleSeatBetting(index)}
                              >
                                {seat.betting ? "個別" : "共通"}
                              </button>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                  <div className="seat-bulk-apply">
                    <label className="field">
                      <span className="field-label">コピー元seat</span>
                      <select
                        className="select"
                        value={Math.min(bulkSeatIndex, settings.seats.length - 1)}
                        onChange={(event) => setBulkSeatIndex(Number(event.target.value))}
                      >
                        {settings.seats.map((seat, index) => (
                          <option key={seat.id} value={index}>{seat.position}</option>
                        ))}
                      </select>
                    </label>
                    <button type="button" className="secondary-button" onClick={applyMultiwaySeatToAll}>
                      stack・range・sizingを全席へ適用
                    </button>
                  </div>
                  <MultiwaySeatOverrides
                    seats={settings.seats}
                    onPreflopChange={updateSeatPreflopSizes}
                    validationProps={fieldValidationProps}
                    onPostflopSizesChange={updateSeatPostflopSizes}
                    onPostflopOptionChange={updateSeatPostflopOption}
                    onRemove={toggleSeatBetting}
                  />

                  <p className="inline-callout">
                    BTNを基準に標準ポジションを自動配置。各レンジは
                    <code>22+,A2s+,KTo+</code> 形式、空欄は全レンジです。
                  </p>
                </div>
              )}
            </section>

            <section className="settings-card" id="tree" aria-labelledby="tree-title">
              <SectionHeading
                number="02"
                title="ベットツリーを組む"
                description={settings.mode === "multiway" ? "プリフロップと各ストリートのサイズ候補・分岐上限を定義します。" : "raise-to サイズと許可する分岐だけを選びます。"}
                badge={`${settings.maxRaises} raises max`}
              />
              <span id="tree-title" className="sr-only">ベットツリーを組む</span>
              <div className="field">
                <span className="field-label">
                  オープンサイズ
                  <span className="field-label-note">{settings.mode === "multiway" ? "preflop raise-to" : "SB open / BB iso 共通"}</span>
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
                      {...fieldValidationProps("preflop-open")}
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

              {settings.mode === "hu" && settings.maxRaises > 1 ? (
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
                  <span className="field-label">{settings.mode === "multiway" ? "Preflop aggressive action cap" : "最大レイズ回数"}</span>
                  <div className="segment-control" aria-label="最大レイズ回数" {...fieldValidationProps("preflop-cap")}>
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
              {settings.mode === "multiway" ? (
                <div className="reveal-panel multiway-sizing">
                  <div className="reveal-title">
                    <strong>All-street bet / raise sizes</strong>
                    <span>comma separated</span>
                  </div>
                  <div className="field-grid">
                    <label className="field">
                      <span className="field-label">
                        PREFLOP isolate
                        <span className="field-label-note">raise-to BB after limp</span>
                      </span>
                      <CommaListInput
                        className="input"
                        values={settings.isolateSizesBb}
                        placeholder="3.5, 4"
                        {...fieldValidationProps("preflop-isolate")}
                        onCommit={(values) => update("isolateSizesBb", values)}
                      />
                    </label>
                    <label className="field">
                      <span className="field-label">
                        PREFLOP re-raise
                        <span className="field-label-note">previous-bet multiple</span>
                      </span>
                      <CommaListInput
                        className="input"
                        values={settings.raiseFactors.flat()}
                        placeholder="3, 2.5"
                        {...fieldValidationProps("preflop-raise")}
                        onCommit={(values) => update("raiseFactors", [values])}
                      />
                    </label>
                  </div>
                  <div className="street-size-grid">
                    {(["flop", "turn", "river"] as const).map((street) => {
                      const streetBetting = settings.multiwayPostflopBetting[street];
                      return (
                        <fieldset className="reveal-panel" key={street}>
                          <legend className="field-label">
                            {street.toUpperCase()}
                          </legend>
                          <div className="bucket-grid">
                            {(["betSizes", "raiseSizes"] as const).map((key) => (
                              <label className="field" key={key}>
                                <span className="field-label">
                                  {key === "betSizes" ? "Bet sizes" : "Raise sizes"}
                                  <span className="field-label-note">pot-after-call</span>
                                </span>
                                <CommaListInput
                                  className="input"
                                  values={streetBetting[key]}
                                  placeholder="0.5, 0.75"
                                  {...fieldValidationProps(`street-${street}-${key}`)}
                                  onCommit={(values) =>
                                    update("multiwayPostflopBetting", {
                                      ...settings.multiwayPostflopBetting,
                                      [street]: {
                                        ...streetBetting,
                                        [key]: values,
                                      },
                                    })
                                  }
                                />
                              </label>
                            ))}
                            <label className="field">
                              <span className="field-label">Aggressive-action cap</span>
                              <input
                                className="input"
                                type="number"
                                min="0"
                                max="16"
                                step="1"
                                {...fieldValidationProps(`street-${street}-cap`)}
                                value={streetBetting.maxAggressiveActions}
                                onChange={(event) =>
                                  update("multiwayPostflopBetting", {
                                    ...settings.multiwayPostflopBetting,
                                    [street]: {
                                      ...streetBetting,
                                      maxAggressiveActions: Math.trunc(
                                        numberValue(event.target.value),
                                      ),
                                    },
                                  })
                                }
                              />
                            </label>
                            <Toggle
                              pressed={streetBetting.includeAllin}
                              onPressedChange={(includeAllin) =>
                                update("multiwayPostflopBetting", {
                                  ...settings.multiwayPostflopBetting,
                                  [street]: { ...streetBetting, includeAllin },
                                })
                              }
                              label="All-inを追加"
                              description={`${street.toUpperCase()}の別分岐`}
                            />
                          </div>
                        </fieldset>
                      );
                    })}
                  </div>
                  <p className="inline-callout">
                    open / isolate / re-raiseと、各streetのbet / raise候補・上限・
                    <code>include_allin</code> を独立して保存します。
                  </p>
                </div>
              ) : null}
            </section>

            <section className="settings-card" id="model" aria-labelledby="model-title">
              <SectionHeading
                number="03"
                title={settings.mode === "multiway" ? "抽象化を配分する" : "継続モデルを選ぶ"}
                description={settings.mode === "multiway" ? "参加人数が減る各局面へ、ストリート別バケット予算を配分します。" : "速度重視か、ポストフロップ近似を含む品質重視か。"}
                badge={settings.mode === "multiway" ? "ALL STREETS" : settings.postflopModel === "equity" ? "FAST" : "EXPERIMENTAL"}
              />
              <span id="model-title" className="sr-only">継続モデルを選ぶ</span>
              {settings.mode === "hu" ? (
                <>
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
                </>
              ) : (
                <div className="reveal-panel">
                  <div className="reveal-title">
                    <strong>Active-opponent bucket profiles</strong>
                    <span>2–{settings.tableSize}-way · preflop 169 fixed</span>
                  </div>
                  <div className="table-scroll">
                    <table className="bucket-profile-table">
                      <caption className="sr-only">
                        参加人数ごとのプリフロップ・フロップ・ターン・リバーバケット
                      </caption>
                      <thead>
                        <tr>
                          <th scope="col">Active</th>
                          <th scope="col">Preflop</th>
                          <th scope="col">Flop</th>
                          <th scope="col">Turn</th>
                          <th scope="col">River</th>
                        </tr>
                      </thead>
                      <tbody>
                        {settings.bucketProfiles.map((profile, index) => (
                          <tr key={profile.activePlayers}>
                            <th scope="row">{profile.activePlayers}-way</th>
                            {(["preflop", "flop", "turn", "river"] as const).map(
                              (street) => (
                                <td key={street}>
                                  <label>
                                    <span className="sr-only">
                                      {profile.activePlayers}-way {street} buckets
                                    </span>
                                    <input
                                      className="input bucket-input"
                                      type="number"
                                      min="1"
                                      max={street === "preflop" ? 169 : 4096}
                                      disabled={street === "preflop"}
                                      step="1"
                                      value={profile[street]}
                                      {...fieldValidationProps(`bucket-${index}-${street}`)}
                                      onChange={(event) =>
                                        updateBucketProfile(
                                          index,
                                          street,
                                          numberValue(event.target.value),
                                        )
                                      }
                                    />
                                  </label>
                                </td>
                              ),
                            )}
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                  <p className="inline-callout">
                    各行はacting playerを除く <code>active_opponents</code> としてv2へ渡され、
                    人数が減った局面のpostflop抽象化へ反映されます。preflopは169クラス固定です。
                  </p>
                </div>
              )}
            </section>
            <section className="settings-card" id="economics" aria-labelledby="economics-title">
              <SectionHeading
                number="04"
                title={settings.mode === "multiway" ? "効用を選ぶ" : "レーキを合わせる"}
                description={settings.mode === "multiway" ? "Cashのchip EV、または外部フィールドを含むTournament ICMを設定します。" : "ゲーム環境のコストをプリフロップ終端へ反映します。"}
                badge={settings.mode === "multiway" ? settings.utilityMode === "icm" ? "TOURNAMENT ICM" : "CHIP EV" : settings.rakeMode === "none" ? "NO RAKE" : "RAKED"}
              />
              <span id="economics-title" className="sr-only">レーキを合わせる</span>
              {settings.mode === "multiway" ? (
                <>
                  <div className="economics-grid utility-grid" aria-label="効用モデル">
                    <button
                      type="button"
                      className="economics-button"
                      aria-pressed={settings.utilityMode === "cash"}
                      onClick={() => setUtilityMode("cash")}
                    >
                      <strong>Cash · chip EV</strong>
                      <small>rake optional</small>
                    </button>
                    <button
                      type="button"
                      className="economics-button"
                      aria-pressed={settings.utilityMode === "icm"}
                      onClick={() => setUtilityMode("icm")}
                    >
                      <strong>Tournament ICM</strong>
                      <small>table + outside field</small>
                    </button>
                  </div>
                  {settings.utilityMode === "icm" ? (
                    <div className="reveal-panel icm-panel">
                      <div className="field-grid">
                        <label className="field">
                          <span className="field-label">
                            Payouts
                            <span className="field-label-note">high → low · paste CSV/lines</span>
                          </span>
                          <textarea
                            className="textarea"
                            rows={7}
                            value={settings.payoutsText}
                            {...fieldValidationProps("icm-payouts")}
                            onChange={(event) => update("payoutsText", event.target.value)}
                            placeholder={"100\n70\n50\n30"}
                          />
                        </label>
                        <label className="field">
                          <span className="field-label">
                            Outside-player stacks
                            <span className="field-label-note">BB · one per line</span>
                          </span>
                          <textarea
                            className="textarea"
                            rows={7}
                            value={settings.outsideStacksText}
                            {...fieldValidationProps("icm-outside")}
                            onChange={(event) => update("outsideStacksText", event.target.value)}
                            placeholder={"24\n31\n48"}
                          />
                        </label>
                      </div>
                      <div className="icm-stats" aria-live="polite">
                        <span><strong>{settings.tableSize + outsidePlayerCount}</strong>remaining</span>
                        <span><strong>{payoutCount}</strong>pasted payouts</span>
                        <span>
                          <strong>
                            {settings.tableSize + outsidePlayerCount <=
                            (bridgeCapabilities?.exactIcmField ?? 15)
                              ? "Exact"
                              : "Sampled"}
                          </strong>
                          auto path
                        </span>
                      </div>
                      <div className="icm-readback-grid" aria-live="polite">
                        <section className="icm-readback-panel" aria-labelledby="outside-readback-title">
                          <h3 id="outside-readback-title">Outside field readback</h3>
                          <div className="table-scroll">
                            <table className="icm-readback-table">
                              <caption className="sr-only">Parsed outside-player stacks</caption>
                              <thead>
                                <tr>
                                  <th scope="col">Player</th>
                                  <th scope="col">Stack</th>
                                </tr>
                              </thead>
                              <tbody>
                                {parsedOutsideStacks.length ? (
                                  parsedOutsideStacks.map((stack, index) => (
                                    <tr key={`outside-${index}`}>
                                      <th scope="row">Field {index + 1}</th>
                                      <td>{stack.toLocaleString()}bb</td>
                                    </tr>
                                  ))
                                ) : (
                                  <tr>
                                    <td colSpan={2}>卓外プレイヤーなし</td>
                                  </tr>
                                )}
                              </tbody>
                            </table>
                          </div>
                        </section>
                        <section className="icm-readback-panel" aria-labelledby="payout-readback-title">
                          <h3 id="payout-readback-title">Payout readback</h3>
                          <div className="table-scroll">
                            <table className="icm-readback-table">
                              <caption className="sr-only">Parsed and zero-padded payouts</caption>
                              <thead>
                                <tr>
                                  <th scope="col">Place</th>
                                  <th scope="col">Payout</th>
                                  <th scope="col">Source</th>
                                </tr>
                              </thead>
                              <tbody>
                                {payoutReadback.map((payout) => (
                                  <tr key={`payout-${payout.place}`}>
                                    <th scope="row">{payout.place}</th>
                                    <td>{payout.amount.toLocaleString()}</td>
                                    <td>{payout.padded ? "0 padded" : "pasted"}</td>
                                  </tr>
                                ))}
                              </tbody>
                            </table>
                          </div>
                        </section>
                      </div>
                      <div className="field-grid three">
                        <label className="field">
                          <span className="field-label">ICM method</span>
                          <select className="select" value="auto" disabled>
                            <option value="auto">Auto (≤15 exact)</option>
                          </select>
                          <p className="helper">
                            15人以下はexact、16人以上はMonte Carloへ自動で切り替わります。
                          </p>
                        </label>
                        <label className="field">
                          <span className="field-label">Samples</span>
                          <input
                            className="input"
                            type="number"
                            min="100"
                            max="1000000"
                            step="100"
                            value={settings.icmSamples}
                            {...fieldValidationProps("icm-samples")}
                            onChange={(event) =>
                              update("icmSamples", Math.trunc(numberValue(event.target.value)))
                            }
                          />
                        </label>
                        <label className="field">
                          <span className="field-label">ICM seed</span>
                          <input
                            className="input"
                            type="number"
                            min="0"
                            step="1"
                            value={settings.icmSeed}
                            {...fieldValidationProps("icm-seed")}
                            onChange={(event) =>
                              update("icmSeed", Math.trunc(numberValue(event.target.value)))
                            }
                          />
                        </label>
                      </div>
                      <p className="inline-callout">
                        未入力の下位payoutはフィールド人数まで0で補完します。最大
                        {bridgeCapabilities?.maxIcmField ?? 100}人、15人以下はautoで
                        exact DPです。ICMとrakeは併用できません。
                      </p>
                    </div>
                  ) : null}
                </>
              ) : null}
              {settings.mode === "hu" || settings.utilityMode === "cash" ? (
                <>



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
                </>
              ) : null}
            </section>
            <section className="settings-card" id="run" aria-labelledby="run-title">
              <SectionHeading number="05" title={settings.mode === "multiway" ? "研究ランを設計する" : "精度と実行を整える"} description={settings.mode === "multiway" ? "External-samplingのsweep予算、再現性、checkpointを指定します。" : "まず既定値で開始し、収束を見て反復数を増やせます。"} badge={settings.storage.toUpperCase()} />
              <span id="run-title" className="sr-only">精度と実行を整える</span>
              {settings.mode === "multiway" ? (
                <div className="multiway-run">
                  <div className="research-warning" role="note">
                    <span aria-hidden="true">!</span>
                    <div>
                      <strong>Research-grade workload</strong>
                      <p>
                        Multiwayの全ストリート解法はリアルタイム用途ではありません。
                        数時間〜数日、数GB以上になる可能性があります。まず小さなpresetで
                        checkpointとメモリ推移を確認してください。
                      </p>
                    </div>
                  </div>
                  <div className="field-grid">
                    <label className="field">
                      <span className="field-label">External-sampling sweeps</span>
                      <input
                        className="input"
                        type="number"
                        min="1"
                        step="10000"
                        value={settings.externalSamplingSweeps}
                        {...fieldValidationProps("run-sweeps")}
                        onChange={(event) =>
                          update(
                            "externalSamplingSweeps",
                            Math.trunc(numberValue(event.target.value)),
                          )
                        }
                      />
                      <p className="helper">
                        1 sweep = 全{settings.tableSize}席を1回ずつ更新。
                      </p>
                    </label>
                    <label className="field">
                      <span className="field-label">Root seed</span>
                      <input
                        className="input"
                        type="number"
                        min="0"
                        step="1"
                        value={settings.externalSamplingSeed}
                        {...fieldValidationProps("run-seed")}
                        onChange={(event) =>
                          update(
                            "externalSamplingSeed",
                            Math.trunc(numberValue(event.target.value)),
                          )
                        }
                      />
                      <p className="helper">再現可能なworld sampling。</p>
                    </label>
                    <label className="field">
                      <span className="field-label">Checkpoint every</span>
                      <span className="input-wrap">
                        <input
                          className="input has-unit"
                          type="number"
                          min="0"
                          step="1000"
                          value={settings.checkpointEvery}
                          {...fieldValidationProps("run-checkpoint")}
                          onChange={(event) =>
                            update(
                              "checkpointEvery",
                              Math.trunc(numberValue(event.target.value)),
                            )
                          }
                        />
                        <span className="unit">SWEEPS</span>
                      </span>
                      <p className="helper">0で自動checkpointを無効化。</p>
                    </label>
                    <label className="field">
                      <span className="field-label">Evaluation cadence</span>
                      <span className="input-wrap">
                        <input
                          className="input has-unit"
                          type="number"
                          min="1"
                          step="1000"
                          value={settings.evaluationCadence}
                          {...fieldValidationProps("run-evaluation-cadence")}
                          onChange={(event) =>
                            update(
                              "evaluationCadence",
                              Math.trunc(numberValue(event.target.value)),
                            )
                          }
                        />
                        <span className="unit">SWEEPS</span>
                      </span>
                      <p className="helper">
                        Checkpointを0にしても独立して評価します。
                      </p>
                    </label>
                    <label className="field">
                      <span className="field-label">Evaluation samples</span>
                      <input
                        className="input"
                        type="number"
                        min="1"
                        max="1000000"
                        step="32"
                        value={settings.evaluationSamples}
                        {...fieldValidationProps("run-evaluation-samples")}
                        onChange={(event) =>
                          update(
                            "evaluationSamples",
                            Math.trunc(numberValue(event.target.value)),
                          )
                        }
                      />
                      <p className="helper">Seat別EV/CIと近似指標のheld-out評価。</p>
                    </label>
                    <label className="field">
                      <span className="field-label">Policy memory limit</span>
                      <span className="input-wrap">
                        <input
                          className="input has-unit"
                          type="number"
                          min="1"
                          max="2048"
                          step="128"
                          value={Math.round(settings.maxMemoryBytes / 1048576)}
                          {...fieldValidationProps("run-memory")}
                          onChange={(event) =>
                            update(
                              "maxMemoryBytes",
                              Math.trunc(numberValue(event.target.value) * 1048576),
                            )
                          }
                        />
                        <span className="unit">MiB</span>
                      </span>
                      <p className="helper">
                        到達時はpolicyをevictせずresource_limitでcheckpoint保存。
                      </p>
                    </label>
                    <label className="field">
                      <span className="field-label">Storage</span>
                      <select className="select" value="f32" disabled>
                        <option value="f32">f32 · fidelity</option>
                      </select>
                      <p className="helper">v2 Multiway bridgeはf32固定です。</p>
                    </label>
                  </div>
                  <label className="field">
                    <span className="field-label">
                      Resume checkpoint
                      <span className="field-label-note">optional managed Bridge URL</span>
                    </span>
                    <input
                      className="input"
                      value={settings.resumeCheckpoint}
                      {...fieldValidationProps("run-resume")}
                      placeholder="/v2/jobs/{id}/checkpoint"
                      spellCheck="false"
                      onChange={(event) =>
                        update("resumeCheckpoint", event.target.value)
                      }
                    />
                    <p className="helper">
                      同一Bridge sessionが発行したmanaged URLを指定し、
                      <code>resumeCheckpointUrl</code>としてjob作成時に送信します。
                    </p>
                  </label>
                  <p className="inline-callout">
                    v2 progressはphase、sweeps、infosets、memory、seat別統計を
                    ストリーム表示します。接続先:
                    <strong>
                      {engineState === "online"
                        ? ` v${bridgeApiVersion} · ${bridgeCapabilities?.maxPlayers ?? 2} seats`
                        : " offline"}
                    </strong>
                  </p>
                </div>
              ) : (
                <>

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
                </>
              )}
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
              <p>{settings.mode === "multiway" ? settings.tableSize + "-max · " + (settings.utilityMode === "icm" ? "Tournament ICM" : "Chip EV") + " · External sampling" : settings.effectiveStackBb + "bb · HU NLHE · " + (settings.postflopModel === "equity" ? "Equity" : "Bucketed")}</p>
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
                  <div className="tree-row"><span className="tree-line" /><strong>{settings.mode === "multiway" ? settings.tableSize + " seats" : "SB starts"}</strong><span>{settings.allowLimp ? "limp · fold" : "fold"}</span></div>
                  <div className="tree-row depth-1"><span className="tree-line" /><strong>Open</strong><span>{settings.openSizesBb.length ? settings.openSizesBb.map((size) => `${size}bb`).join(" · ") : "jam only"}</span></div>
                  {settings.maxRaises > 1 ? <div className="tree-row depth-2"><span className="tree-line" /><strong>3-bet</strong><span>{settings.raiseFactors.flat().join("× · ") || "all-in only"}{settings.raiseFactors.flat().length ? "×" : ""}</span></div> : null}
                  {settings.maxRaises > 2 ? <div className="tree-row depth-2"><span className="tree-line" /><strong>4-bet+</strong><span>{settings.raiseFactors[1]?.[0] ?? 2.5}×</span></div> : null}
                  {settings.includeAllin ? <div className="tree-row depth-1"><span className="tree-line" /><strong>Jam</strong><span>each level</span></div> : null}
                </div>
              </div>

              {job?.status === "running" || job?.status === "cancelling" ? (
                <div className="summary-section" aria-live="polite">
                  <p className="summary-section-title">SOLVE PROGRESS <span>{job.status === "cancelling" ? "CANCELLING" : job.progress ? progressCurrent.toLocaleString() + (jobMode === "multiway" ? " sweeps" : " iter") : "PREPARING"}</span></p>
                  <div className="range-bar" role="progressbar" aria-label="Solve progress" aria-valuemin={0} aria-valuemax={progressTarget} aria-valuenow={progressCurrent}>
                    <span style={{ width: progressPercent + "%" }} />
                  </div>
                  <p className="helper">
                    {!job.progress
                      ? "ゲームツリーとsampling worldを準備しています。"
                      : jobMode === "multiway"
                        ? (job.progress.phase ?? "sampling") +
                          " · " +
                          (job.progress.infosets ?? 0).toLocaleString() +
                          " infosets · " +
                          ((job.progress.memoryBytes ?? 0) / 1048576).toFixed(1) +
                          " MiB"
                        : "NashConv " +
                          (job.progress.nashConv ?? 0).toFixed(5) +
                          " · " +
                          (job.progress.elapsedSecs ?? 0).toFixed(1) + "s"}
                  </p>
                  {jobMode === "multiway" && (job.apiVersion ?? bridgeApiVersion) === 2 ? (
                    <button
                      type="button"
                      className="danger-button cancel-button"
                      disabled={job.status === "cancelling"}
                      onClick={() => void cancelSolve()}
                    >
                      {job.status === "cancelling" ? "キャンセル中…" : "解析をキャンセル"}
                    </button>
                  ) : null}
                  {job.checkpointUrl ? <button type="button" className="text-button" onClick={() => void navigator.clipboard.writeText(job.checkpointUrl ?? "")}>Checkpoint URLをコピー</button> : null}
                </div>
              ) : null}
                  {jobMode === "multiway" &&
                  job?.progress?.seats?.length ? (
                    <ul className="seat-progress" aria-label="Seat progress">
                      {job?.progress?.seats?.map((seat) => {
                        const position =
                          jobPositions[seat.seat] ??
                          `Seat ${seat.seat + 1}`;
                        return (
                          <li key={seat.seat}>
                            <strong>{position}</strong>
                            <span>
                              {seat.profileEv
                                ? `EV ${seat.profileEv.mean.toFixed(4)} · `
                                : "EV pending · "}
                              regret {seat.averagePositiveRegret.toFixed(5)} · drift{" "}
                              {seat.strategyDriftL1.toFixed(5)}
                            </span>
                          </li>
                        );
                      })}
                    </ul>
                  ) : null}


              <div
                className="summary-section"
                id={VALIDATION_SUMMARY_ID}
                role={validationErrors.length ? "alert" : "status"}
                aria-live="polite"
              >
                <p className="summary-section-title">CHECKS <span>{validationErrors.length ? `${validationErrors.length} ISSUES` : "READY"}</span></p>
                <ul
                  id={VALIDATION_MESSAGES_ID}
                  className={`validation-list ${validationErrors.length ? "error" : ""}`}
                >
                  {validationErrors.length ? (
                    validationErrors.slice(0, 3).map((error) => (
                      <li key={error}><span className="validation-mark">!</span><span>{translateValidation(error)}</span></li>
                    ))
                  ) : (
                    <>
                      <li><span className="validation-mark">✓</span><span>0.1bbチップグリッドに変換可能</span></li>
                      <li><span className="validation-mark">✓</span><span>{settings.mode === "multiway" ? "全席のスタックとレンジを検証済み" : "両プレイヤーのレンジに正の質量あり"}</span></li>
                      <li><span className="validation-mark">✓</span><span>{estimate.quality}</span></li>
                    </>
                  )}
                </ul>
              </div>

              <div className="summary-actions">
                <button
                  type="button"
                  className="primary-button"
                  aria-describedby={VALIDATION_SUMMARY_ID}
                  disabled={validationErrors.length > 0 || job?.status === "running" || job?.status === "cancelling" || engineBusy}
                  onClick={() => void startSolve()}
                >{primaryLabel}</button>
                <button type="button" className="secondary-button" onClick={() => void copyToml()}>TOMLをコピー</button>
              </div>
              <p className="engine-hint">
                {settings.mode === "multiway"
                  ? engineState === "online" && bridgeApiVersion >= 2
                    ? "v2認証済み。設定・checkpoint・結果はこのPC内だけで処理されます。"
                    : "Multiway直接実行にはv2 bridgeが必要です。"
                  : settings.postflopModel === "bucketed"
                    ? "Bucketedは長時間実行のため、TOMLを書き出してCLIから開始します。"
                    : engineState === "online"
                      ? "認証済みloopback接続。設定はこのPC内だけで処理されます。"
                      : "直接実行するには "}
              </p>
            </div>
          </div>
        </aside>
      </div>

      {modal === "connection" ? (
        <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setModal(null);
        }}>
          <section ref={modalRef} className="modal" role="dialog" aria-modal="true" aria-labelledby="connection-title">
            <header className="modal-head">
              <div><h2 id="connection-title">ローカルソルバーへ接続</h2><p>ローカル開発では <code>preflop-solver serve</code> を起動します。公開URLでは <code>--origin</code> にこのページのOriginを指定してください。</p></div>
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
          <section ref={modalRef} className="modal range-modal" role="dialog" aria-modal="true" aria-labelledby="range-title">
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
          <section ref={modalRef} className="modal" role="dialog" aria-modal="true" aria-labelledby="review-title">
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
          <section ref={modalRef} className="modal result-modal" role="dialog" aria-modal="true" aria-labelledby="result-title">
            <header className="modal-head">
              <div><h2 id="result-title">{multiwayResult ? "Multiway解析結果" : "PF解析が完了しました"}</h2><p>{multiwayResult ? "Seat・履歴・nodeを選び、近似profileと169クラス戦略を探索できます。" : "169クラスのroot戦略を含むsolver resultです。"}</p></div>
              <button type="button" className="icon-button" onClick={() => setModal(null)} aria-label="閉じる">×</button>
            </header>
            <div className="modal-body">
              {multiwayResult ? (
                <MultiwayResultExplorer
                  result={multiwayResult}
                  strategies={strategyBlocks}
                  positions={jobPositions}
                  hasMore={Boolean(strategyCursor)}
                  loading={strategyBusy}
                  onLoadMore={() => { if (job && strategyCursor) void loadStrategyPage(job.id, strategyCursor); }}
                />
              ) : (
                <pre className="code-preview">{resultJson}</pre>
              )}
            </div>
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
