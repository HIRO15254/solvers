import { useState } from "react";
import { EmptyState } from "../cryst";
import { significant } from "../lib/format";

const W = 420;
const H = 200;
const PAD = { left: 48, right: 16, top: 12, bottom: 28 };

/**
 * Exploitability against iteration on a log y axis. Points are drawn where
 * they were measured: P1's automatic cadence evaluates at uneven intervals
 * (docs/app-architecture.md §5.1), so nothing here assumes a fixed step.
 */
export function ConvergenceChart({
  title,
  points,
  target,
  unit,
}: {
  title: string;
  points: { x: number; y: number }[];
  target: number | null;
  unit: string;
}) {
  const [hover, setHover] = useState<number | null>(null);
  const shown = points.filter((p) => p.y > 0 && Number.isFinite(p.y));
  const tipPoint = shown[hover ?? shown.length - 1];
  return (
    <div className="chart-panel">
      <div className="row wrap" style={{ alignItems: "baseline" }}>
        <h3>{title}</h3>
        <span className="grow" />
        {tipPoint && (
          <span className="mono meta muted">
            iteration {tipPoint.x} · {significant(tipPoint.y)}
            {unit}
          </span>
        )}
      </div>
      {shown.length === 0 ? (
        <EmptyState size="sm" icon="chart-line" title="No evaluation points yet" />
      ) : (
        <Plot points={shown} target={target} unit={unit} onHover={setHover} />
      )}
    </div>
  );
}

function Plot({
  points,
  target,
  unit,
  onHover,
}: {
  points: { x: number; y: number }[];
  target: number | null;
  unit: string;
  onHover: (index: number | null) => void;
}) {
  const ys = points.map((p) => p.y).concat(target && target > 0 ? [target] : []);
  const top = Math.ceil(Math.log10(Math.max(...ys)));
  const bottom = Math.min(Math.floor(Math.log10(Math.min(...ys))), top - 1);
  const maxX = Math.max(...points.map((p) => p.x), 1);
  const x = (v: number) => PAD.left + (v / maxX) * (W - PAD.left - PAD.right);
  const y = (v: number) => PAD.top + ((top - Math.log10(v)) / (top - bottom)) * (H - PAD.top - PAD.bottom);
  const decades = Array.from({ length: top - bottom + 1 }, (_, i) => top - i);
  const line = points.map((p) => `${x(p.x).toFixed(1)},${y(p.y).toFixed(1)}`).join(" ");
  return (
    <svg
      className="chart"
      viewBox={`0 0 ${W} ${H}`}
      role="img"
      aria-label="NashConv / 2 by iteration"
      onMouseLeave={() => onHover(null)}
    >
      {decades.map((d) => (
        <g key={d}>
          <line className="grid" x1={PAD.left} x2={W - PAD.right} y1={y(10 ** d)} y2={y(10 ** d)} />
          <text className="tick" x={PAD.left - 6} y={y(10 ** d) + 4} textAnchor="end">
            {significant(10 ** d)}
            {unit}
          </text>
        </g>
      ))}
      {[0, 0.5, 1].map((f) => (
        <text key={f} className="tick" x={x(maxX * f)} y={H - 8} textAnchor="middle">
          {Math.round(maxX * f)}
        </text>
      ))}
      {target && target > 0 && (
        <g>
          <line className="target" x1={PAD.left} x2={W - PAD.right} y1={y(target)} y2={y(target)} />
          <text className="label" x={W - PAD.right} y={y(target) - 6} textAnchor="end">
            Target {significant(target)}
            {unit}
          </text>
        </g>
      )}
      <polyline className="series" points={line} />
      {points.map((p, i) => (
        <g key={p.x}>
          <circle className="point" cx={x(p.x)} cy={y(p.y)} r={3} />
          <circle className="hit" cx={x(p.x)} cy={y(p.y)} r={10} onMouseEnter={() => onHover(i)} />
        </g>
      ))}
    </svg>
  );
}
