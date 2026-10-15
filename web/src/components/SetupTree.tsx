import { Fragment, useId, useMemo, useState } from "react";
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
import { CodeEditor, highlightScript } from "./CodeEditor";
import { DraftInput, Field, InfoHint, Notice } from "./common";
import { DocText, FieldIssue, useDoc, useIssue } from "./DocFields";

const VIEWS = [
  { id: "sizes", label: "Size table", icon: "table" },
  { id: "script", label: "Script", icon: "code" },
];

const STREET_ORDER = ["preflop", "flop", "turn", "river"];
const ROLE_LABEL: Record<SizeRole, string> = { oop: "OOP", ip: "IP" };

const SIZE_HELP =
  "33 is 33% of the pot, 2.5x is 2.5 times the largest wager on the street (for raises), e reaches all-in geometrically over the streets left, a is all-in, 3bb is a total of 3 BB, min is the minimum. Separate with commas or spaces; an empty cell leaves that action out.";

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
}: {
  /** The street the spot starts on; earlier rows do not reach the tree. */
  startStreet: string | null;
  roles: Record<SizeRole, string> | null;
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
  const issueLine = Number(/^line (\d+): /.exec(scriptIssue?.message ?? "")?.[1] ?? 0);
  const issueCell = table && issueLine > 0 ? sizeCellAt(script, issueLine) : null;
  const cellIssue = scriptIssue?.message.replace(/^line \d+: /, "");

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
            <Field label="Script" hint="The tree grammar in docs/nlh-input-v1.jp.md §9" htmlFor={scriptId}>
              <CodeEditor
                id={scriptId}
                value={script}
                highlight={highlightScript}
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

      <Card title="All-in and aggression limits">
        <div className="form-sections">
          <div className="stack" style={{ gap: 16 }}>
            <div className="form-grid">
              <DocText
                label="All-in merge threshold"
                path={["tree", "allin_threshold"]}
                placeholder="none"
                hint="Merge a size into all-in when it is at least this share of the largest amount (above 0, at most 1)"
              />
            </div>
            <Checkbox
              label="Add all-in at every node (include_allin)"
              checked={getAt(doc, ["tree", "include_allin"]) === true}
              onCheckedChange={(checked) => update(["tree", "include_allin"], checked === true || undefined)}
            />
            <FieldIssue keys={["tree.include_allin"]} />
          </div>
          <div className="form-grid">
            {SIZE_STREETS.map((street) => (
              <DocText
                key={street}
                label={`${STREET_LABEL[street]} limit`}
                path={["tree", "max_aggressive_actions", street]}
                placeholder="3"
              />
            ))}
          </div>
        </div>
      </Card>
    </>
  );
}
