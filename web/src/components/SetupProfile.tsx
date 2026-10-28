import { useState } from "react";
import type { TomlTable, TomlPrimitive } from "smol-toml";
import { Button, Card, Input, Select } from "../cryst";
import { fieldText, fieldValue, getAt, setAt } from "../lib/config";
import { changeProfileEntry, replaceProfileLocks } from "../lib/profile";
import { DraftInput } from "./common";
import { useDoc } from "./DocFields";

function table(value: unknown): TomlTable {
  return value && typeof value === "object" && !Array.isArray(value) && !(value instanceof Date) ? value as TomlTable : {};
}


function NumberField({ label, value, onChange }: { label: string; value: unknown; onChange: (value: TomlPrimitive) => void }) {
  return <DraftInput label={label} value={fieldText(value)} read={(text) => fieldText(fieldValue(text) ?? "")} onText={(text) => onChange(fieldValue(text) ?? "")} />;
}

function Strategy({ value, onChange }: { value: unknown; onChange: (value: TomlTable) => void }) {
  const entries = Object.entries(table(value));
  return <div className="stack" style={{ gap: 8 }}>
    {entries.map(([action, probability], index) => <div className="row" key={index}>
      <Input label="Action label" value={action} onChange={(e) => {
        const next = [...entries]; next[index] = [e.target.value, probability]; onChange(Object.fromEntries(next));
      }} />
      <NumberField label="Probability" value={probability} onChange={(v) => {
        const next = [...entries]; next[index] = [action, v]; onChange(Object.fromEntries(next));
      }} />
      <Button variant="outline" onClick={() => onChange(Object.fromEntries(entries.filter((_, i) => i !== index)))}>Remove action</Button>
    </div>)}
    <Button variant="outline" disabled={entries.some(([key]) => key === "")} onClick={() => onChange({ ...table(value), "": 0 })}>Add action</Button>
  </div>;
}

function Entry({ kind, entry, index, onChange, onRemove }: { kind: string; entry: TomlTable; index: number; onChange: (entry: TomlTable) => void; onRemove: () => void }) {
  const set = (key: string, value: TomlPrimitive | undefined) => {
    const next = { ...entry }; if (value === undefined) delete next[key]; else next[key] = value; onChange(next);
  };
  const combos = kind === "lock" && Object.hasOwn(entry, "combos");
  const selector = Object.hasOwn(entry, "category") ? "category" : Object.hasOwn(entry, "hands") ? "hands" : "all";
  return <Card title={`${kind === "lock" ? "Lock" : kind === "incentive" ? "Incentive" : "Frequency target"} ${index + 1}`}>
    <div className="stack" style={{ gap: 12 }}>
      <Input label="Node selector" placeholder="x b3.3 or [?]" value={fieldText(entry.node)} onChange={(e) => set("node", e.target.value)} />
      {kind === "lock" && <Select label="Lock form" value={combos ? "combos" : "strategy"} options={[{ value: "strategy", label: "Hand selection and strategy" }, { value: "combos", label: "Per-combo strategies" }]} onValueChange={(value) => {
        const next = { ...entry }; delete next.strategy; delete next.combos; delete next.hands; delete next.category;
        if (value === "combos") next.combos = {}; else next.strategy = {};
        onChange(next);
      }} />}
      {!combos && <>
        <Select label="Hands" value={selector} options={[{ value: "all", label: "All hands" }, { value: "hands", label: "Range" }, { value: "category", label: "Category expression" }]} onValueChange={(value) => {
          const next = { ...entry }; delete next.hands; delete next.category;
          if (value !== "all") next[value] = ""; onChange(next);
        }} />
        {selector !== "all" && <Input label={selector === "hands" ? "Hand range" : "Category expression"} placeholder={selector === "hands" ? "AA,KK,AKs" : "toppair & !flushdraw"} value={fieldText(entry[selector])} onChange={(e) => set(selector, e.target.value)} />}
      </>}
      {kind === "lock" && <>
        <NumberField label="Confidence (default 1)" value={entry.confidence} onChange={(v) => set("confidence", v === "" ? undefined : v)} />
        {combos ? <ComboStrategies value={entry.combos} onChange={(v) => set("combos", v)} /> : <Strategy value={entry.strategy} onChange={(v) => set("strategy", v)} />}
      </>}
      {kind !== "lock" && <Input label="Action label" value={fieldText(entry.action)} onChange={(e) => set("action", e.target.value)} />}
      {kind === "incentive" && <>
        <label>Bonus unit <select value={Object.hasOwn(entry, "bb") ? "bb" : "pot"} onChange={(e) => {
          const next = { ...entry }; delete next.pot; delete next.bb; next[e.target.value] = 0; onChange(next);
        }}><option value="pot">Fraction of pot</option><option value="bb">BB</option></select></label>
        <NumberField label="Bonus" value={entry[Object.hasOwn(entry, "bb") ? "bb" : "pot"]} onChange={(v) => set(Object.hasOwn(entry, "bb") ? "bb" : "pot", v)} />
      </>}
      {kind === "frequency" && <NumberField label="Target frequency" value={entry.target} onChange={(v) => set("target", v)} />}
      <Button variant="outline" onClick={onRemove}>Remove {kind}</Button>
    </div>
  </Card>;
}

function ComboStrategies({ value, onChange }: { value: unknown; onChange: (value: TomlTable) => void }) {
  const entries = Object.entries(table(value));
  return <div className="stack" style={{ gap: 12 }}>
    {entries.map(([combo, strategy], index) => <div className="stack" style={{ gap: 8 }} key={index}>
      <Input label="Combo" placeholder="AsKs" value={combo} onChange={(e) => {
        const next = [...entries]; next[index] = [e.target.value, strategy]; onChange(Object.fromEntries(next));
      }} />
      <Strategy value={strategy} onChange={(v) => { const next = [...entries]; next[index] = [combo, v]; onChange(Object.fromEntries(next)); }} />
      <Button variant="outline" onClick={() => onChange(Object.fromEntries(entries.filter((_, i) => i !== index)))}>Remove combo</Button>
    </div>)}
    <Button variant="outline" disabled={entries.some(([key]) => key === "")} onClick={() => onChange({ ...table(value), "": {} })}>Add combo</Button>
  </div>;
}

export function SetupProfile({ prefill = null }: { prefill?: { node: string; combos?: Record<string, Record<string, number>> } | null }) {
  const { doc, setDoc } = useDoc();
  const [confirmReplace, setConfirmReplace] = useState(false);
  const [imported, setImported] = useState(false);
  const locks = getAt(doc, ["profile", "lock"]);
  const importStrategy = () => {
    if (!prefill?.combos) return;
    setDoc((d) => replaceProfileLocks(d, prefill.node, prefill.combos!));
    setConfirmReplace(false);
    setImported(true);
  };
  return <div className="stack" style={{ gap: 12 }}>
    {prefill?.combos && <Card title="Current strategy" description={`Import the strategy from node ${prefill.node} as a per-combo lock.`}>
      {imported && <p role="status">Current strategy imported as a lock.</p>}
      {confirmReplace ? <>
        <p role="alert">Existing locks may overlap this node, including wildcard selectors. Importing will replace all existing locks.</p>
        <div className="row">
          <Button onClick={importStrategy}>Replace all locks and import</Button>
          <Button variant="outline" onClick={() => setConfirmReplace(false)}>Cancel</Button>
        </div>
      </> : <Button variant="outline" onClick={() => {
        if (Array.isArray(locks) && locks.length > 0) setConfirmReplace(true);
        else importStrategy();
      }}>Lock current strategy</Button>}
    </Card>}
    <Card title="Opponent profile" description="Locks constrain individual hands; incentives change action utility; frequency targets apply to the reach-weighted hand selection per runout.">
      <Input label="Freeze upstream from solution (optional)" value={fieldText(getAt(doc, ["profile", "freeze_upstream", "solution"]))} onChange={(e) => setDoc((d) => setAt(d, ["profile", "freeze_upstream", "solution"], e.target.value || undefined))} />
      <p className="muted meta">The daemon cannot submit a config with an upstream solution path. Remove this setting to solve in the GUI; it can be saved for CLI use.</p>
    </Card>
    {(["lock", "incentive", "frequency"] as const).map((kind) => {
      const value = getAt(doc, ["profile", kind]); const entries = Array.isArray(value) ? value : [];
      return <div className="stack" style={{ gap: 12 }} key={kind}>
        {entries.map((entry, index) => <Entry key={index} kind={kind} index={index} entry={table(entry)} onChange={(next) => setDoc((d) => changeProfileEntry(d, kind, index, next))} onRemove={() => setDoc((d) => changeProfileEntry(d, kind, index, null))} />)}
        <Button variant="outline" onClick={() => setDoc((d) => changeProfileEntry(d, kind, entries.length, kind === "lock" ? { node: prefill?.node ?? "", strategy: {} } : kind === "incentive" ? { node: prefill?.node ?? "", action: "", pot: 0 } : { node: prefill?.node ?? "", action: "", target: 0 }))}>Add {kind === "frequency" ? "frequency target" : kind}</Button>
      </div>;
    })}
  </div>;
}
