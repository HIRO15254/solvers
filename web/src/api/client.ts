import type {
  ArtifactEntry,
  CreateRunResponse,
  ErrorCode,
  EventPage,
  LineCursor,
  PreviewTreeRow,
  RelockRequest,
  RunSummary,
  ServerInfo,
  ValidateResponse,
} from "./types";

const TOKEN_KEY = "solversd.token";

/** A daemon error response, or a transport failure (`code` undefined). */
export class ApiError extends Error {
  readonly status: number;
  readonly code: ErrorCode | undefined;

  constructor(status: number, code: ErrorCode | undefined, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

/**
 * Picks up a token passed as `#token=...` (the URL `solversd --ui` prints),
 * stores it for this origin, and removes it from the address bar so it does
 * not linger in history or screenshots.
 */
export function adoptTokenFromLocation(): void {
  const match = /(?:^#|&)token=([^&]+)/.exec(window.location.hash);
  if (!match?.[1]) return;
  localStorage.setItem(TOKEN_KEY, decodeURIComponent(match[1]));
  const rest = window.location.hash.replace(/(^#|&)token=[^&]+/, "$1").replace(/^#&/, "#");
  history.replaceState(null, "", window.location.pathname + window.location.search + (rest === "#" ? "" : rest));
}

export function storedToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? "";
}

export function storeToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

async function request(path: string, init: RequestInit = {}): Promise<Response> {
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${storedToken()}`);
  let response: Response;
  try {
    response = await fetch(path, { ...init, headers });
  } catch (error) {
    throw new ApiError(0, undefined, `Cannot connect to daemon: ${(error as Error).message}`);
  }
  if (!response.ok) {
    let code: ErrorCode | undefined;
    let message = `${response.status} ${response.statusText}`;
    try {
      const body = (await response.json()) as { code?: ErrorCode; message?: string };
      code = body.code;
      // Config errors are the CLI's stderr, which starts with its "Error: ".
      if (body.message) message = body.message.replace(/^Error: /, "");
    } catch {
      // Not a daemon error body; keep the status line.
    }
    throw new ApiError(response.status, code, message);
  }
  return response;
}

async function json<T>(path: string, init?: RequestInit): Promise<T> {
  return (await request(path, init)).json() as Promise<T>;
}

function post<T>(path: string, body?: unknown): Promise<T> {
  return json<T>(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

const run = (id: string) => `/v1/runs/${encodeURIComponent(id)}`;

export const api = {
  info: () => json<ServerInfo>("/v1"),
  validate: (configToml: string, resources: boolean) =>
    post<ValidateResponse>("/v1/validate", { configToml, resources }),
  /**
   * Rows of the tree the config defines, before solving (`POST /v1/tree`); `nodes` are selectors, default the root.
   * `restOfStreet` follows each selected node with the action nodes after it on its street.
   */
  tree: (configToml: string, nodes: string[] = [], restOfStreet = false) =>
    post<PreviewTreeRow[]>("/v1/tree", { configToml, nodes, restOfStreet }),
  /** Replays `moves` (or, without them, the config's `spot.line`) and reports the next legal moves. */
  line: (configToml: string, moves?: string[]) =>
    post<LineCursor>("/v1/line", moves === undefined ? { configToml } : { configToml, moves }),
  runs: async () => (await json<{ runs: RunSummary[] }>("/v1/runs")).runs,
  run: (id: string) => json<RunSummary>(run(id)),
  createRun: (configToml: string, runId?: string) =>
    post<CreateRunResponse>("/v1/runs", runId ? { configToml, runId } : { configToml }),
  relock: (id: string, body: RelockRequest) =>
    post<CreateRunResponse>(`${run(id)}/relock`, body),
  events: (id: string, from: number) => json<EventPage>(`${run(id)}/events?from=${from}`),
  cancel: (id: string) => post<RunSummary>(`${run(id)}/cancel`),
  resume: (id: string) => post<CreateRunResponse>(`${run(id)}/resume`),
  artifacts: async (id: string) => (await json<{ artifacts: ArtifactEntry[] }>(`${run(id)}/artifacts`)).artifacts,
  artifactText: async (id: string, name: string) =>
    (await request(`${run(id)}/artifacts/${encodeURIComponent(name)}`)).text(),
  artifactBlob: async (id: string, name: string) =>
    (await request(`${run(id)}/artifacts/${encodeURIComponent(name)}`)).blob(),
  /** A solution view rendered by `solvers export` on the daemon's host. */
  solution: <T>(id: string, view: string, nodes: string[] = [], options: SolutionOptions = {}) =>
    json<T>(`${run(id)}/solution/${view}${solutionQuery(nodes, options, false)}`),
  solutionCsv: async (id: string, view: string, nodes: string[] = [], options: SolutionOptions = {}) =>
    (await request(`${run(id)}/solution/${view}${solutionQuery(nodes, options, true)}`)).blob(),
};

interface SolutionOptions {
  /** Re-solve the River nodes a `no-rivers` run did not store (`resolve=rivers`): one River solve per request. */
  resolveRivers?: boolean;
  /** Follow each node with the action nodes after it on its street (`restOfStreet=true`). */
  restOfStreet?: boolean;
}

function solutionQuery(nodes: string[], options: SolutionOptions, csv: boolean): string {
  const params = nodes.map((n) => `node=${encodeURIComponent(n)}`);
  if (options.resolveRivers) params.push("resolve=rivers");
  if (options.restOfStreet) params.push("restOfStreet=true");
  if (csv) params.push("format=csv");
  return params.length === 0 ? "" : `?${params.join("&")}`;
}

/** Saves a blob through the browser's download prompt. */
export function saveBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}
