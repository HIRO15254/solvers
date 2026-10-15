import { useEffect, useMemo, useState, type ReactNode } from "react";
import { api, saveBlob } from "../api/client";
import { isTerminal, type ArtifactEntry, type HuProgressRow, type RunEvent, type RunSummary } from "../api/types";
import { Board, KeyValue, Notice, StateBadge } from "../components/common";
import { ConvergenceChart } from "../components/ConvergenceChart";
import {
  Button,
  EmptyState,
  Icon,
  Input,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Tabs,
} from "../cryst";
import { bytes, clock, integer, seconds, significant } from "../lib/format";
import { exploitability, exploitabilityPercent, runMeta, targetPercent, type RunMeta } from "../lib/runMeta";
import { navigate } from "../route";

type Filter = "all" | "active" | "done";

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "active", label: "Running" },
  { id: "done", label: "Finished" },
];

/** Loads the display metadata of every listed run, keyed by config hash. */
function useRunMetas(runs: RunSummary[]): Record<string, RunMeta> {
  const [metas, setMetas] = useState<Record<string, RunMeta>>({});
  useEffect(() => {
    for (const run of runs) {
      if (metas[run.configHash] || run.configHash === "pending") continue;
      runMeta(run)
        .then((meta) => setMetas((current) => ({ ...current, [run.configHash]: meta })))
        .catch(() => undefined);
    }
  }, [runs, metas]);
  return metas;
}

export function RunsPage({ runId, maxConcurrent }: { runId?: string; maxConcurrent?: number }) {
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [tick, setTick] = useState(0);
  const metas = useRunMetas(runs);

  useEffect(() => {
    let live = true;
    const load = async () => {
      try {
        const list = await api.runs();
        if (!live) return;
        setRuns(list.reverse());
        setError(null);
      } catch (e) {
        if (live) setError((e as Error).message);
      }
    };
    void load();
    const timer = setInterval(load, 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [tick]);

  const shown = runs.filter((run) => {
    if (filter === "active" && isTerminal(run.state)) return false;
    if (filter === "done" && !isTerminal(run.state)) return false;
    const meta = metas[run.configHash];
    const haystack = `${run.runId} ${meta?.line ?? ""} ${meta?.name ?? ""} ${meta?.board.join("") ?? ""}`.toLowerCase();
    return haystack.includes(query.toLowerCase());
  });
  const selected = runs.find((run) => run.runId === runId) ?? null;

  return (
    <>
      <header className="page-header">
        <h1>Runs</h1>
        <span className="grow" />
        {maxConcurrent !== undefined && (
          <span className="meta-item">
            <Icon name="cpu" />
            Concurrent runs <span className="mono">{maxConcurrent}</span>
          </span>
        )}
        <Button variant="outline" icon="refresh" onClick={() => setTick((t) => t + 1)}>
          Refresh
        </Button>
        <Button icon="plus" onClick={() => navigate({ page: "setup" })}>
          New run
        </Button>
      </header>
      <div className="split">
        <section className="split-main">
          <div className="toolbar">
            <div style={{ flex: "0 1 240px", minWidth: 0 }}>
              <Input
                icon="search"
                size="sm"
                placeholder="Search runs"
                aria-label="Search runs"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <Tabs
              variant="pill"
              items={FILTERS}
              value={filter}
              onValueChange={(value) => setFilter(FILTERS.find((f) => f.id === value)?.id ?? "all")}
            />
          </div>
          {error && (
            <div className="toolbar">
              <Notice variant="destructive" className="grow" title="Cannot load runs" description={error} />
            </div>
          )}
          <div className="scroll">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Status</TableHead>
                  <TableHead>run</TableHead>
                  <TableHead>line</TableHead>
                  <TableHead numeric>iteration</TableHead>
                  <TableHead numeric>NashConv / 2</TableHead>
                  <TableHead numeric>Elapsed</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {shown.map((run) => {
                  const meta = metas[run.configHash];
                  const nashConv = run.lastProgress?.nash_conv;
                  return (
                    <TableRow
                      key={run.runId}
                      className="clickable"
                      selected={run.runId === runId}
                      onClick={() => navigate({ page: "runs", runId: run.runId })}
                    >
                      <TableCell>
                        <StateBadge state={run.state} />
                      </TableCell>
                      <TableCell>
                        <span className="mono" style={{ fontWeight: 500 }}>
                          {run.runId}
                        </span>
                      </TableCell>
                      <TableCell>
                        <span className="mono muted nowrap">
                          {meta
                            ? `${meta.line || "Preflop"}${meta.board.length ? ` · ${meta.board.join("")}` : ""}`
                            : ""}
                        </span>
                      </TableCell>
                      <TableCell numeric>{run.progress !== undefined ? integer(run.progress) : ""}</TableCell>
                      <TableCell numeric>
                        {nashConv !== undefined && run.gameKind === "hu-postflop" ? exploitability(nashConv, meta) : ""}
                      </TableCell>
                      <TableCell numeric>{run.elapsedSecs !== undefined ? seconds(run.elapsedSecs) : ""}</TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
            {shown.length === 0 && !error && <EmptyState size="sm" icon="list-details" title="No runs" />}
          </div>
        </section>
        {selected && (
          <RunDetail
            key={selected.runId}
            run={selected}
            meta={metas[selected.configHash]}
            onChanged={() => setTick((t) => t + 1)}
          />
        )}
      </div>
    </>
  );
}

function parseProgress(text: string): HuProgressRow[] {
  return text
    .split("\n")
    .filter((line) => line.trim())
    .flatMap((line) => {
      try {
        return [JSON.parse(line) as HuProgressRow];
      } catch {
        return []; // An unfinished final line: the writer is mid-append.
      }
    });
}

function eventText(event: RunEvent): string {
  switch (event.kind) {
    case "state":
      return event.state;
    case "checkpoint":
      return `iteration ${integer(event.sweeps)}`;
    case "stop":
      return event.reason;
    case "notice":
    case "failure":
      return event.message;
  }
}

function RunDetail({ run, meta, onChanged }: { run: RunSummary; meta?: RunMeta; onChanged: () => void }) {
  const [events, setEvents] = useState<RunEvent[]>([]);
  const [progress, setProgress] = useState<HuProgressRow[]>([]);
  const [artifacts, setArtifacts] = useState<ArtifactEntry[]>([]);
  const [actionError, setActionError] = useState<string | null>(null);
  const terminal = isTerminal(run.state);
  const hu = run.gameKind === "hu-postflop";

  // Follow the event log by byte offset; a terminal page ends the stream,
  // and a resume (terminal → running) starts it over.
  useEffect(() => {
    let live = true;
    let offset = 0;
    setEvents([]);
    let timer: number | undefined;
    const poll = async () => {
      try {
        const page = await api.events(run.runId, offset);
        if (!live) return;
        offset = page.nextOffset;
        if (page.events.length) setEvents((current) => [...current, ...page.events]);
        if (page.terminal) return;
      } catch {
        // Retried on the next tick.
      }
      if (live) timer = window.setTimeout(poll, 1500);
    };
    void poll();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [run.runId, terminal]);

  // Re-read progress and artifacts while the run moves; once when it is done.
  useEffect(() => {
    let live = true;
    const load = async () => {
      const [text, list] = await Promise.all([
        hu ? api.artifactText(run.runId, "progress.jsonl").catch(() => "") : Promise.resolve(""),
        api.artifacts(run.runId).catch(() => []),
      ]);
      if (!live) return;
      setProgress(parseProgress(text));
      setArtifacts(list);
    };
    void load();
    if (terminal) return () => void (live = false);
    const timer = setInterval(load, 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [run.runId, terminal, hu]);

  const act = async (action: () => Promise<unknown>) => {
    setActionError(null);
    try {
      await action();
      onChanged();
    } catch (error) {
      setActionError((error as Error).message);
    }
  };

  const last = progress.at(-1);
  const points = useMemo(
    () =>
      progress.map((row) => ({ x: row.iteration, y: exploitabilityPercent(row.nash_conv, meta) ?? row.nash_conv / 2 })),
    [progress, meta],
  );
  const hasSolution = artifacts.some((a) => a.name === "solution.sol");
  const chartUnit = exploitabilityPercent(1, meta) !== null ? "%" : "";
  const title = `NashConv / 2${chartUnit ? `（% ${meta?.utilityUnit === "prizes" ? "prizes" : "pot"}）` : ""}`;
  // With a normalizer the chart title carries the unit, so the stat shows only the figure.
  const lastPercent = last ? exploitabilityPercent(last.nash_conv, meta) : null;
  // A stop event after the latest start means the solver is past its last
  // iteration and writing checkpoint/solution; that can take tens of seconds.
  const lastStart = events.findLastIndex((event) => event.kind === "state" && event.state === "running");
  const stopping = !terminal && events.findLastIndex((event) => event.kind === "stop") > lastStart;

  return (
    <aside className="split-detail" aria-label="Run details">
      <div className="row" style={{ gap: 8 }}>
        <h2 className="mono detail-title" title={run.runId}>
          {run.runId}
        </h2>
        <StateBadge state={run.state} />
        <span className="grow" />
        {stopping ? (
          <span className="meta-item">
            <Icon name="loader-2" className="spin" />
            Stopping (saving)
          </span>
        ) : (
          !terminal && (
            <Button variant="outline" size="sm" icon="player-stop" onClick={() => act(() => api.cancel(run.runId))}>
              Cancel
            </Button>
          )
        )}
        {run.resumable && (
          <Button variant="outline" size="sm" icon="player-play" onClick={() => act(() => api.resume(run.runId))}>
            Resume
          </Button>
        )}
        {hu && hasSolution && (
          <Button
            variant="outline"
            size="sm"
            icon="chart-grid-dots"
            onClick={() => navigate({ page: "results", runId: run.runId })}
          >
            Results
          </Button>
        )}
      </div>
      {actionError && <Notice variant="destructive" title="Action failed" description={actionError} />}
      {run.failure && <Notice variant="destructive" title="Run failed" description={run.failure} />}
      {meta && (
        <div className="row wrap" style={{ gap: 8 }}>
          {meta.name && <span>{meta.name}</span>}
          <span className="mono muted">{meta.line || "Preflop root"}</span>
          <Board cards={meta.board} small />
        </div>
      )}
      <div className="stats">
        <Stat label="Iterations">{run.progress !== undefined ? integer(run.progress) : "—"}</Stat>
        <Stat label="Elapsed">{run.elapsedSecs !== undefined ? seconds(run.elapsedSecs) : "—"}</Stat>
        {hu && (
          <Stat label="NashConv / 2">
            {last
              ? lastPercent !== null
                ? `${significant(lastPercent)}%`
                : exploitability(last.nash_conv, meta)
              : "—"}
          </Stat>
        )}
      </div>
      {hu && <ConvergenceChart title={title} points={points} target={targetPercent(meta)} unit={chartUnit} />}
      <div className="stack" style={{ gap: 6 }}>
        <KeyValue label="Stopping target">{meta?.target ?? "None"}</KeyValue>
        {meta?.resources && (
          <>
            <KeyValue label="Nodes">{integer(meta.resources.nodes)}</KeyValue>
            <KeyValue label="Storage">
              {meta.resources.storage} · {bytes(meta.resources.memoryEstimateBytes)}
            </KeyValue>
          </>
        )}
        <KeyValue label="Threads">{meta?.threads ?? "—"}</KeyValue>
        {meta?.economics && <KeyValue label="Economics">{meta.economics}</KeyValue>}
        {run.completion && <KeyValue label="Completion reason">{run.completion}</KeyValue>}
      </div>
      <section className="section">
        <h3 className="overline">events</h3>
        {events.map((event) => (
          <div key={event.seq} className="event-row mono">
            <span className="muted">{event.seq}</span>
            <span className="muted">{clock(event.unixMs)}</span>
            <span style={{ color: event.level === "error" ? "var(--destructive)" : undefined }}>{event.kind}</span>
            <span className="muted event-text">{eventText(event)}</span>
          </div>
        ))}
        {events.length === 0 && <span className="muted">None</span>}
      </section>
      <section className="section">
        <h3 className="overline">artifacts</h3>
        <div className="row wrap" style={{ gap: 6 }}>
          {artifacts.map((artifact) => (
            <Button
              key={artifact.name}
              variant="outline"
              size="sm"
              icon="download"
              title={bytes(artifact.bytes)}
              onClick={async () => saveBlob(await api.artifactBlob(run.runId, artifact.name), artifact.name)}
            >
              {artifact.name}
            </Button>
          ))}
        </div>
      </section>
    </aside>
  );
}

function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="stat">
      <span>{label}</span>
      <span className="mono">{children}</span>
    </div>
  );
}
