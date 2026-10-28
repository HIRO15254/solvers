import { Fragment, useId, useMemo, useState } from "react";
import type { TreeParam } from "../api/types";
import { Button, Card, CardAction, CardContent, CardHeader, CardTitle, Checkbox, Tabs } from "../cryst";
import { STREET_LABEL } from "../lib/cards";
import { fieldText, getAt } from "../lib/config";
import {
  DEFAULT_SIZES,
  SIZE_KINDS,
  SIZE_LITERAL,
  SIZE_ROLES,
  SIZE_STREETS,
  anyOopBet,
  anyReraise,
  readSizeScript,
  sizeCellAt,
  sizeList,
  sizeScript,
  type SizeKind,
  type SizeRole,
  type SizeStreet,
  type SizeTable,
} from "../lib/sizes";
import { scriptIssueLine } from "../lib/editor/diagnostics";
import { CodeEditor } from "./CodeEditor";
import { DraftInput, Field, InfoHint, Notice } from "./common";
import { FieldIssue, useDoc, useIssue } from "./DocFields";

const VIEWS = [
  { id: "sizes", label: "Size table", icon: "table" },
  { id: "script", label: "Script", icon: "code" },
];

const STREET_ORDER = ["preflop", "flop", "turn", "river"];
const ROLE_LABEL: Record<SizeRole, string> = { oop: "OOP", ip: "IP" };

const SIZE_HELP =
  "33 is 33% of the pot, 2.5x is 2.5 times the largest wager on the street (for raises), e reaches all-in geometrically over the streets left, a is all-in, 3bb is a total of 3 BB, min is the minimum. Separate with commas or spaces; an empty cell leaves that action out.";

const SCRIPT_HELP =
  "The tree grammar in docs/nlh-input-v1.jp.md §9. Ctrl+Space completes keywords, variables and sizes; hover a word for its meaning. Ctrl+F searches, Ctrl+/ toggles a comment, Ctrl+[ / Ctrl+] indents, Ctrl+Shift+[ folds a block. Tab indents; press Esc, then Tab, to leave the editor.";

function withSizes(table: SizeTable, street: SizeStreet, role: SizeRole, kind: SizeKind, list: string[]): SizeTable {
  const sizes = table.sizes[street];
  return {
    ...table,
    sizes: {
      ...table.sizes,
      [street]: { ...sizes, [role]: { ...sizes[role], [kind]: list } },
    },
  };
}

/**
 * The Tree step. Bet and raise sizes are edited as a table (street × role)
 * that writes `tree.script`; a script the table cannot hold is edited as a
 * script, and the table can replace it with its defaults.
 */
export function SetupTree({
  startStreet,
  roles,
  params,
}: {
  /** The street the spot starts on; earlier rows do not reach the tree. */
  startStreet: string | null;
  roles: Record<SizeRole, string> | null;
  params: TreeParam[];
}) {
  const { doc, update } = useDoc();
  const scriptId = useId();
  const script = fieldText(getAt(doc, ["tree", "script"]));
  const table = useMemo(() => readSizeScript(script), [script]);
  const [view, setView] = useState(() => (table ? "sizes" : "script"));
  const write = (next: SizeTable) => update(["tree", "script"], sizeScript(next) || undefined);
  const unused = (street: string) =>
    startStreet !== null && STREET_ORDER.indexOf(street) < STREET_ORDER.indexOf(startStreet);
  // A script error names its line (`line 11: …`); in the table it goes under the cell that wrote that line.
  const scriptIssue = useIssue("tree.script");
  const located = scriptIssue ? scriptIssueLine(scriptIssue.message) : null;
  const issueLine = located?.line ?? 0;
  const issueCell = table && issueLine > 0 ? sizeCellAt(script, issueLine) : null;
  const cellIssue = located?.message ?? scriptIssue?.message;
  const scriptDiagnostics = useMemo(
    () => (located ? [{ line: located.line, message: located.message, severity: "error" as const }] : []),
    [located?.line, located?.message],
  );
  const overrides = getAt(doc, ["tree", "params"]);
  const extraParams =
    typeof overrides === "object" && overrides !== null && !Array.isArray(overrides)
      ? Object.keys(overrides).filter((name) => !params.some((param) => param.name === name))
      : [];

  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>Bet sizes</CardTitle>
          <CardAction>
            <Tabs variant="pill" items={VIEWS} value={view} onValueChange={setView} />
          </CardAction>
        </CardHeader>
        <CardContent>
          {view === "script" ? (
            <Field label="Script" hint={SCRIPT_HELP} htmlFor={scriptId}>
              <CodeEditor
                id={scriptId}
                label="Script"
                language="script"
                value={script}
                diagnostics={scriptDiagnostics}
                onChange={(value) => update(["tree", "script"], value.trim() ? value : undefined)}
              />
              <FieldIssue keys={["tree.script"]} />
            </Field>
          ) : table === null ? (
            <div className="stack" style={{ gap: 8 }}>
              <Notice
                variant="info"
                title="The size table cannot show this script"
                description="It has rules the table cannot hold, such as conditions. Edit it on the Script tab, or replace it with the default size table."
                actions={
                  <Button size="sm" variant="outline" icon="replace" onClick={() => write(DEFAULT_SIZES)}>
                    Replace with the default size table
                  </Button>
                }
              />
              <FieldIssue keys={["tree.script"]} />
            </div>
          ) : (
            <div className="stack" style={{ gap: 16 }}>
              <div className="size-table" role="group" aria-label="Bet and raise sizes">
                <span className="field-label">
                  <span className="muted meta">Size</span>
                  <InfoHint label="Size" text={SIZE_HELP} />
                </span>
                {SIZE_ROLES.map((role) => (
                  <span key={role} className="size-role">
                    {ROLE_LABEL[role]}
                    {roles && <span className="mono muted">{roles[role]}</span>}
                  </span>
                ))}
                <span />
                {SIZE_ROLES.flatMap((role) =>
                  SIZE_KINDS.map((kind) => (
                    <span key={role + kind} className="muted meta">
                      {kind}
                    </span>
                  )),
                )}
                {SIZE_STREETS.map((street) => (
                  <Fragment key={street}>
                    <span
                      className={`size-street ${unused(street) ? "muted" : ""}`}
                      title={unused(street) ? "Before the starting street, so not in the tree" : undefined}
                    >
                      {STREET_LABEL[street]}
                    </span>
                    {SIZE_ROLES.flatMap((role) =>
                      SIZE_KINDS.map((kind) => (
                        <DraftInput
                          key={role + kind}
                          size="sm"
                          className="mono"
                          aria-label={`${STREET_LABEL[street]} ${ROLE_LABEL[role]} ${kind}`}
                          value={table.sizes[street][role][kind].join(", ")}
                          issue={
                            issueCell?.street === street && issueCell.role === role && issueCell.kind === kind
                              ? cellIssue
                              : undefined
                          }
                          placeholder="none"
                          spellCheck={false}
                          read={(text) => sizeList(text).join(", ")}
                          onText={(text) => {
                            const list = sizeList(text);
                            if (!list.every((size) => SIZE_LITERAL.test(size)))
                              return "Has characters a size cannot use";
                            write(withSizes(table, street, role, kind, list));
                          }}
                        />
                      )),
                    )}
                  </Fragment>
                ))}
              </div>
              {scriptIssue && !issueCell && <FieldIssue keys={["tree.script"]} />}
              <div className="stack" style={{ gap: 8 }}>
                <Checkbox
                  label="No donk bets (OOP does not lead into the last bettor or raiser of the previous street)"
                  checked={table.noDonk}
                  disabled={!anyOopBet(table)}
                  onCheckedChange={(checked) => write({ ...table, noDonk: checked })}
                />
                <Checkbox
                  label="Re-raises are all-in only (a raise facing a bet and a raise)"
                  checked={table.reraiseAllIn}
                  disabled={!anyReraise(table)}
                  onCheckedChange={(checked) => write({ ...table, reraiseAllIn: checked })}
                />
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {params.length > 0 && (
        <Card title="Script parameters">
          <div className="form-grid">
            {params.map((param) => (
              <ScriptParam key={param.name} param={param} />
            ))}
          </div>
        </Card>
      )}
      {extraParams.length > 0 && (
        <Notice
          variant="warning"
          title="Other parameter overrides"
          description="These overrides have no declaration in the last validated script. Remove unused overrides, or fix the script and validate again."
        >
          <div className="stack" style={{ gap: 8 }}>
            {extraParams.map((name) => (
              <div className="row" key={name}>
                <span className="mono">{name} = {fieldText(getAt(doc, ["tree", "params", name]))}</span>
                <Button size="sm" variant="outline" icon="trash" onClick={() => update(["tree", "params", name], undefined)}>
                  Remove
                </Button>
                <FieldIssue keys={[`tree.params.${name}`]} />
              </div>
            ))}
          </div>
        </Notice>
      )}
      <FieldIssue keys={["tree.params"]} />
    </>
  );
}

function ScriptParam({ param }: { param: TreeParam }) {
  const { doc, update } = useDoc();
  const id = useId();
  const path = ["tree", "params", param.name];
  const override = getAt(doc, path);
  const issue = useIssue(path.join("."));
  const value = (text: string) => {
    if (text.trim() === "") return undefined;
    return param.kind === "number" ? Number(text) : text;
  };
  return (
    <div className="stack" style={{ gap: 8 }}>
      <Field label={param.name} hint={param.description ?? `Script default: ${param.value}`} htmlFor={id}>
        {param.kind === "bool" ? (
          <Checkbox
            id={id}
            label={override === undefined ? `Default: ${param.value}` : fieldText(override)}
            checked={override === undefined ? param.value === "true" : override === true}
            onCheckedChange={(checked) => update(path, checked)}
          />
        ) : (
          <DraftInput
            id={id}
            className="mono"
            value={fieldText(override)}
            placeholder={param.value}
            spellCheck={false}
            read={(text) => fieldText(value(text))}
            issue={issue?.message}
            onText={(text) => {
              const next = value(text);
              if (typeof next === "number" && !Number.isFinite(next)) return "Enter a number";
              update(path, next);
            }}
          />
        )}
        {param.kind === "bool" && <FieldIssue keys={[path.join(".")]} />}
      </Field>
      {override !== undefined && (
        <Button size="sm" variant="outline" onClick={() => update(path, undefined)}>
          Use script default
        </Button>
      )}
    </div>
  );
}
