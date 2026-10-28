import { useEffect, useState } from "react";
import { api } from "../api/client";
import type { ProfileReport, StrategyRow, TreeRow } from "../api/types";
import { Button, Card, CardContent, Checkbox, Input, Select } from "../cryst";
import { classesToRange, relockPayload, roundedStrategy, selectionStrategy, type PendingLock } from "../lib/profile";
import { ALL_CLASSES } from "../lib/range";
import { navigate } from "../route";

export function RelockPanel({ runId, history, row, strategy, profile, config }: {
  runId: string; history: string; row: TreeRow | undefined; strategy: StrategyRow[] | undefined;
  profile: ProfileReport | undefined; config: Record<string, unknown> | null;
}) {
  const [locks, setLocks] = useState<PendingLock[]>([]);
  const [replace, setReplace] = useState(false);
  const [freeze, setFreeze] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");
  const node = history || "root";
  const existing = (profile?.summary.resolved_nodes ?? []).filter((entry) => entry.history === history);
  const submit = async () => {
    setSubmitting(true); setError("");
    try {
      const result = await api.relock(runId, relockPayload(locks, replace, freeze));
      navigate({ page: "runs", runId: result.runId });
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setSubmitting(false); }
  };
  return <Card><CardContent className="stack" style={{ gap: 12 }}>
    <strong>Lock &amp; re-solve</strong>
    {row?.kind === "action" && strategy ? <LockEditor key={node} node={node} actions={row.actions} rows={strategy} onAdd={(lock) => setLocks((current) => [...current, lock])} /> : <span className="muted">Select a loaded action node to add a lock.</span>}
    <div><strong>Existing entries at this node</strong>
      {existing.length === 0 ? <div className="muted">None</div> : existing.map((entry, i) => {
        const entries = config?.[entry.kind];
        const value = Array.isArray(entries) && entry.entry !== null ? entries[entry.entry] : undefined;
        return <div key={i}><code>{entry.kind}</code> {value === undefined ? "(details unavailable)" : JSON.stringify(value)}</div>;
      })}
    </div>
    <Checkbox label="Replace existing locks with the same node selector" checked={replace} onCheckedChange={(v) => setReplace(v === true)} />
    <span className="muted meta">Replacement applies to queued lock selectors exactly; wildcard entries matching this node are not removed unless their selector is identical.</span>
    <Select label="Upstream" value={freeze ? "freeze" : "resolve"} options={[{ value: "resolve", label: "Re-solve whole tree" }, { value: "freeze", label: "Freeze upstream to this solution" }]} onValueChange={(v) => setFreeze(v === "freeze")} />
    <strong>Pending locks ({locks.length})</strong>
    {locks.map((lock, index) => <div className="row wrap" key={index}><span className="mono">{lock.node} · {lock.hands ?? lock.category ?? "all hands"} · {Object.entries(lock.strategy).map(([a, p]) => `${a}: ${p}`).join(", ")}</span><Button variant="outline" onClick={() => setLocks((current) => current.filter((_, i) => i !== index))}>Remove</Button></div>)}
    {error && <div role="alert">{error}</div>}
    <Button disabled={!locks.length || submitting} onClick={submit}>{submitting ? "Submitting…" : "Re-solve queued locks"}</Button>
  </CardContent></Card>;
}

function LockEditor({ node, actions, rows, onAdd }: { node: string; actions: string[]; rows: StrategyRow[]; onAdd: (lock: PendingLock) => void }) {
  const [mode, setMode] = useState("all");
  const [classes, setClasses] = useState<Set<string>>(new Set());
  const [category, setCategory] = useState("");
  const [confidence, setConfidence] = useState("");
  const [values, setValues] = useState<string[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    const probabilities = selectionStrategy(rows, actions, mode === "grid" ? classes : undefined);
    setValues(roundedStrategy(probabilities));
  }, [rows, actions, mode, classes]);
  const add = () => {
    const probs = values.map((v) => v.trim() === "" ? NaN : Number(v));
    const c = confidence.trim() === "" ? undefined : Number(confidence);
    if (mode === "grid" && !classes.size) return setError("Select at least one hand class.");
    if (mode === "category" && !category.trim()) return setError("Enter a category expression.");
    if (probs.length !== actions.length || probs.some((p) => !Number.isFinite(p) || p < 0 || p > 1) || Math.abs(probs.reduce((a, b) => a + b, 0) - 1) > 0.00001) return setError("Action probabilities must be between 0 and 1 and sum to 1.");
    if (c !== undefined && (!Number.isFinite(c) || c < 0 || c > 1)) return setError("Confidence must be between 0 and 1.");
    const lock: PendingLock = { node, strategy: Object.fromEntries(actions.map((action, i) => [action, probs[i]!])) };
    if (mode === "grid") lock.hands = classesToRange(classes);
    if (mode === "category") lock.category = category.trim();
    if (c !== undefined) lock.confidence = c;
    onAdd(lock); setError("");
  };
  return <div className="stack" style={{ gap: 10 }}>
    <span>Node <code>{node}</code></span>
    <Select label="Hands" value={mode} options={[{ value: "all", label: "All hands" }, { value: "grid", label: "Select hand classes" }, { value: "category", label: "Category expression" }]} onValueChange={setMode} />
    {mode === "grid" && <><div aria-label="Select hand classes" style={{ display: "grid", gridTemplateColumns: "repeat(13, minmax(0, 1fr))", maxWidth: 520, gap: 2 }}>{ALL_CLASSES.map((name) => <button type="button" key={name} aria-pressed={classes.has(name)} onClick={() => setClasses((old) => { const next = new Set(old); if (next.has(name)) next.delete(name); else next.add(name); return next; })} style={{ padding: "5px 0", fontSize: 11, background: classes.has(name) ? "var(--primary, #4169a1)" : "transparent", color: classes.has(name) ? "white" : "inherit", border: "1px solid var(--border, #777)", borderRadius: 3 }}>{name}</button>)}</div><span className="mono meta">{classesToRange(classes) || "No classes selected"}</span></>}
    {mode === "category" && <><Input label="Category expression" placeholder="toppair & !flushdraw" value={category} onChange={(e) => setCategory(e.target.value)} /><span className="muted meta">The initial strategy is the whole-node average; category matching is resolved by the solver.</span></>}
    <div className="row wrap" style={{ gap: 8 }}>{actions.map((action, i) => <Input key={action} label={`${action} probability`} type="number" min="0" max="1" step="any" value={values[i] ?? ""} onChange={(e) => setValues((old) => old.map((value, j) => j === i ? e.target.value : value))} />)}</div>
    <Input label="Confidence (optional, default 1)" type="number" min="0" max="1" step="any" value={confidence} onChange={(e) => setConfidence(e.target.value)} />
    {error && <div role="alert">{error}</div>}
    <Button variant="outline" onClick={add}>Queue lock</Button>
  </div>;
}
