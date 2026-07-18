"use client";

import { useMemo, useState } from "react";
import {
  compactBytes,
  dominantAction,
  historyId,
  nodeId,
  streetLabel,
  type MultiwayResultV2,
  type MultiwayStrategyBlock,
} from "./multiway-result";

const RANKS = ["A", "K", "Q", "J", "T", "9", "8", "7", "6", "5", "4", "3", "2"];
const HANDS = RANKS.flatMap((rowRank, row) =>
  RANKS.map((columnRank, column) => {
    if (row === column) return `${rowRank}${columnRank}`;
    return row < column
      ? `${rowRank}${columnRank}s`
      : `${columnRank}${rowRank}o`;
  }),
);

interface Props {
  result: MultiwayResultV2;
  strategies: MultiwayStrategyBlock[];
  positions: string[];
  hasMore: boolean;
  loading: boolean;
  onLoadMore: () => void;
}

function shortHistory(value: string): string {
  if (/^0+$/.test(value)) return "root";
  return `${value.slice(0, 8)}…${value.slice(-4)}`;
}

function privatePathId(block: MultiwayStrategyBlock): string {
  return block.key.bucket_path.slice(0, block.key.street).join("/");
}

function privatePathLabel(value: string): string {
  if (!value) return "Preflop root";
  return value
    .split("/")
    .map(Number)
    .map((bucket, street) =>
      street === 0
        ? `PF ${HANDS[bucket] ?? `#${bucket}`}`
        : `${streetLabel(street)} B${bucket}`,
    )
    .join(" · ");
}

function historyLabel(
  block: MultiwayStrategyBlock | undefined,
  positions: string[],
): string {
  if (!block) return "unknown";
  if (!block.public_history?.length) return shortHistory(historyId(block));
  return block.public_history
    .map((event) => `${positions[event.actor] ?? `Seat ${event.actor + 1}`} ${event.action}`)
    .join(" → ");
}
function estimateLabel(
  estimate: { mean: number; stderr: number; ci95: [number, number] } | null | undefined,
): string {
  if (!estimate) return "pending";
  return `${estimate.mean.toFixed(4)} [${estimate.ci95[0].toFixed(4)}, ${estimate.ci95[1].toFixed(4)}]`;
}

export default function MultiwayResultExplorer({
  result,
  strategies,
  positions,
  hasMore,
  loading,
  onLoadMore,
}: Props) {
  const [seat, setSeat] = useState(0);
  const [history, setHistory] = useState("");
  const [node, setNode] = useState("");
  const [privatePath, setPrivatePath] = useState("");
  const [bucket, setBucket] = useState(0);

  const seatStrategies = useMemo(
    () => strategies.filter((block) => block.key.actor === seat),
    [seat, strategies],
  );
  const histories = useMemo(
    () => [...new Set(seatStrategies.map(historyId))],
    [seatStrategies],
  );
  const selectedHistory = histories.includes(history) ? history : (histories[0] ?? "");
  const historyStrategies = useMemo(
    () =>
      seatStrategies.filter((block) => historyId(block) === selectedHistory),
    [seatStrategies, selectedHistory],
  );
  const nodes = useMemo(
    () => [...new Set(historyStrategies.map(nodeId))],
    [historyStrategies],
  );
  const selectedNode = nodes.includes(node) ? node : (nodes[0] ?? "");
  const nodeStrategies = useMemo(
    () => historyStrategies.filter((block) => nodeId(block) === selectedNode),
    [historyStrategies, selectedNode],
  );
  const street = nodeStrategies[0]?.key.street ?? 0;
  const privatePaths = useMemo(
    () => [...new Set(nodeStrategies.map(privatePathId))],
    [nodeStrategies],
  );
  const selectedPrivatePath = privatePaths.includes(privatePath)
    ? privatePath
    : (privatePaths[0] ?? "");
  const pathStrategies = useMemo(
    () =>
      nodeStrategies.filter(
        (block) => privatePathId(block) === selectedPrivatePath,
      ),
    [nodeStrategies, selectedPrivatePath],
  );
  const byBucket = useMemo(
    () =>
      new Map(
        pathStrategies.map((block) => [
          block.key.bucket_path[block.key.street] ?? 0,
          block,
        ]),
      ),
    [pathStrategies],
  );
  const displayBuckets = useMemo(
    () =>
      street === 0
        ? HANDS.map((label, index) => ({ index, label }))
        : [...byBucket.keys()]
            .sort((left, right) => left - right)
            .map((index) => ({ index, label: `B${index}` })),
    [byBucket, street],
  );
  const selectedBucket = byBucket.has(bucket)
    ? bucket
    : (displayBuckets[0]?.index ?? 0);
  const selectedBlock = byBucket.get(selectedBucket);
  const nodeExample = pathStrategies[0] ?? nodeStrategies[0];
  const seatMetric = result.seats.find((metric) => metric.seat === seat);

  return (
    <div className="result-explorer">
      <div className="approximation-notice" role="note">
        <strong>Approximate multiway profile</strong>
        <p>{result.approximationNotice}</p>
      </div>

      <div className="result-kpis" aria-label="Solve diagnostics">
        <div><span>STATUS</span><strong>{result.status.replace("_", " ")}</strong></div>
        <div><span>SWEEPS</span><strong>{result.sweeps.toLocaleString()}</strong></div>
        <div><span>INFOSETS</span><strong>{result.infosets.toLocaleString()}</strong></div>
        <div><span>MEMORY</span><strong>{compactBytes(result.memoryBytes)}</strong></div>
        <div><span>THROUGHPUT</span><strong>{Math.round(result.traversalsPerSecond).toLocaleString()}/s</strong></div>
      </div>

      <div className="result-selectors">
        <label className="field">
          <span className="field-label">Seat</span>
          <select
            className="select"
            value={seat}
            onChange={(event) => {
              setSeat(Number(event.target.value));
              setHistory("");
              setNode("");
              setPrivatePath("");
              setBucket(0);
            }}
          >
            {result.seats.map((metric) => (
              <option key={metric.seat} value={metric.seat}>
                {positions[metric.seat] ?? `Seat ${metric.seat + 1}`}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field-label">Public history</span>
          <select
            className="select"
            value={selectedHistory}
            onChange={(event) => {
              setHistory(event.target.value);
              setNode("");
              setPrivatePath("");
              setBucket(0);
            }}
          >
            {histories.length ? (
              histories.map((value) => (
                <option key={value} value={value}>{historyLabel(seatStrategies.find((block) => historyId(block) === value), positions)}</option>
              ))
            ) : (
              <option value="">No loaded histories</option>
            )}
          </select>
        </label>
        <label className="field">
          <span className="field-label">Node</span>
          <select
            className="select"
            value={selectedNode}
            onChange={(event) => {
              setNode(event.target.value);
              setPrivatePath("");
              setBucket(0);
            }}
          >
            {nodes.length ? (
              nodes.map((value) => {
                const block = historyStrategies.find((item) => nodeId(item) === value);
                return (
                  <option key={value} value={value}>
                    {block
                      ? `${streetLabel(block.key.street)} · ${block.key.active_opponents + 1}-way`
                      : value}
                  </option>
                );
              })
            ) : (
              <option value="">No loaded nodes</option>
            )}
          </select>
        </label>
        {street > 0 ? (
          <label className="field">
            <span className="field-label">Private recall path</span>
            <select
              className="select"
              value={selectedPrivatePath}
              onChange={(event) => {
                setPrivatePath(event.target.value);
                setBucket(0);
              }}
            >
              {privatePaths.map((value) => (
                <option key={value} value={value}>{privatePathLabel(value)}</option>
              ))}
            </select>
          </label>
        ) : null}
      </div>

      <div className="seat-diagnostics">
        <div>
          <span>Profile EV · mean [95% CI]</span>
          <strong>{estimateLabel(seatMetric?.profileEv)}</strong>
        </div>
        <div>
          <span>Deviation gain lower bound</span>
          <strong>{estimateLabel(seatMetric?.deviationGainLowerBound)}</strong>
        </div>
        <div>
          <span>Average positive regret</span>
          <strong>{(seatMetric?.averagePositiveRegret ?? 0).toFixed(5)}</strong>
        </div>
        <div>
          <span>Strategy drift L1</span>
          <strong>{(seatMetric?.strategyDriftL1 ?? 0).toFixed(5)}</strong>
        </div>
      </div>

      <div className="strategy-layout">
        <section className="strategy-matrix-panel" aria-labelledby="strategy-grid-title">
          <div className="result-subhead">
            <div>
              <h3 id="strategy-grid-title">{street === 0 ? "13×13 preflop strategy" : `${streetLabel(street)} abstraction strategy`}</h3>
              <p>
                {nodeExample
                  ? `${positions[nodeExample.key.actor] ?? `Seat ${nodeExample.key.actor + 1}`} · ${streetLabel(nodeExample.key.street)} · ${pathStrategies.length}${street === 0 ? "/169 classes" : " current-street buckets"} loaded`
                  : "Select or load a strategy node."}
              </p>
            </div>
            {hasMore ? (
              <button
                type="button"
                className="secondary-button compact-button"
                disabled={loading}
                onClick={onLoadMore}
              >
                {loading ? "Loading…" : "Load 100 more"}
              </button>
            ) : null}
          </div>
          <div className="strategy-grid" role="grid" aria-label={street === 0 ? "169 hand-class strategy grid" : `${streetLabel(street)} abstraction bucket strategy grid`}>
            {displayBuckets.map(({ index, label }) => {
              const block = byBucket.get(index);
              const dominant = dominantAction(block);
              return (
                <button
                  key={index}
                  type="button"
                  role="gridcell"
                  className={`strategy-cell action-${(dominant?.index ?? 0) % 5}`}
                  aria-selected={selectedBucket === index}
                  aria-label={
                    dominant
                      ? `${label}: ${dominant.action} ${(dominant.probability * 100).toFixed(1)} percent`
                      : `${label}: strategy not loaded`
                  }
                  disabled={!block}
                  onClick={() => setBucket(index)}
                  style={{
                    backgroundSize: `${Math.round((dominant?.probability ?? 0) * 100)}% 100%`,
                  }}
                >
                  <strong>{label}</strong>
                  <span>{dominant ? `${Math.round(dominant.probability * 100)}%` : "—"}</span>
                </button>
              );
            })}
          </div>
        </section>

        <aside className="action-breakdown" aria-label="Selected hand action mix">
          <div className="result-subhead">
            <div>
              <h3>{street === 0 ? (HANDS[selectedBucket] ?? "Class") : `${streetLabel(street)} B${selectedBucket}`} actions</h3>
              <p>Structured labels from the v2 strategy artifact.</p>
            </div>
          </div>
          {selectedBlock ? (
            <ul>
              {selectedBlock.actions.map((action, index) => {
                const probability = selectedBlock.probabilities[index] ?? 0;
                return (
                  <li key={action + index}>
                    <span><strong>{action}</strong><em>{(probability * 100).toFixed(2)}%</em></span>
                    <span className="action-probability" aria-hidden="true">
                      <span style={{ width: `${probability * 100}%` }} />
                    </span>
                  </li>
                );
              })}
            </ul>
          ) : (
            <p className="inline-callout">
              This bucket is not in the loaded page. Load more strategy blocks
              or choose an available cell.
            </p>
          )}
          <dl className="node-key">
            <div><dt>History</dt><dd>{selectedBlock ? historyLabel(selectedBlock, positions) : "—"}</dd></div>
            <div><dt>Bucket path</dt><dd>{selectedBlock?.key.bucket_path.join(" / ") ?? "—"}</dd></div>
            <div><dt>Active opponents</dt><dd>{selectedBlock?.key.active_opponents ?? "—"}</dd></div>
          </dl>
        </aside>
      </div>
    </div>
  );
}
