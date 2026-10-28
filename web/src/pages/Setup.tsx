import { useEffect, useMemo, useRef, useState } from "react";
import { TomlError } from "smol-toml";
import { api, saveBlob } from "../api/client";
import type { LineCursor, TreeParam } from "../api/types";
import { BoardDialog } from "../components/BoardDialog";
import { CodeEditor } from "../components/CodeEditor";
import { Notice } from "../components/common";
import {
  DocContext,
  DocSelect,
  DocText,
  FieldIssue,
  parseIssue,
  useDoc,
  useIssue,
  type Issue,
} from "../components/DocFields";
import { RangeEditor } from "../components/RangeEditor";
import { SetupProfile } from "../components/SetupProfile";
import { SetupStart } from "../components/SetupStart";
import { SpotCard, ValidationCard, type Validation } from "../components/SetupSummary";
import { SetupTable } from "../components/SetupTable";
import { SetupTree } from "../components/SetupTree";
import { TreeCard } from "../components/TreePreview";
import { TemplateDialog } from "../components/TemplateDialog";
import {
  Badge,
  Button,
  Card,
  CardAction,
  CardContent,
  CardHeader,
  CardTitle,
  Checkbox,
  EmptyState,
  Icon,
  Tabs,
  TabsList,
  TabsTrigger,
} from "../cryst";
import { parseBoard } from "../lib/cards";
import { TEMPLATES, fieldText, getAt, readDoc, setAt, writeDoc, type Doc } from "../lib/config";
import { type LineDiagnostic, tomlIssueLine } from "../lib/editor/diagnostics";
import type { SizeRole } from "../lib/sizes";
import { navigate } from "../route";

const DRAFT_KEY = "solvers.setup.draft";
const DEFAULT_TEMPLATE = "flop_srp";

type Step = "table" | "start" | "range" | "tree" | "profile" | "solver" | "toml";

/**
 * The steps of a spot, in the order GTO Wizard's custom-solution builder,
 * GTO+ and the step sidebar of b-inary/wasm-postflop ask for them (the idea
 * only: wasm-postflop is AGPL and no code of it is used, see AGENTS.md):
 * table, history and board, ranges, bet sizes, solver.
 */
const STEPS: { id: Step; label: string; icon: string }[] = [
  { id: "table", label: "Table", icon: "users" },
  { id: "start", label: "Start", icon: "route" },
  { id: "range", label: "Range", icon: "grid-dots" },
  { id: "tree", label: "Tree", icon: "binary-tree" },
  { id: "profile", label: "Profile", icon: "lock" },
  { id: "solver", label: "Solver", icon: "cpu" },
];

/** Not a step: the whole document as text, set apart at the right of the steps. */
const TOML_STEP = { id: "toml", label: "TOML", icon: "code" } as const;

/** The step that edits each top-level key, so a validation error can lead to it. */
const KEY_STEP: Record<string, Step> = {
  schema: "toml",
  meta: "toml",
  table: "table",
  economics: "table",
  spot: "start",
  ranges: "range",
  tree: "tree",
  profile: "profile",
  solver: "solver",
  run: "solver",
  output: "solver",
};

/** Every key path the steps have a field for; the rest of a document is kept untouched. */
const FORM_KEYS = new Set([
  "schema",
  "meta.name",
  "table.players",
  "table.stack_bb",
  "table.sb_bb",
  "table.ante_bb",
  "table.bb_ante_bb",
  "table.straddles_bb",
  "economics.kind",
  "economics.rake.rate",
  "economics.rake.cap_bb",
  "economics.rake.when",
  "economics.payouts",
  "economics.outside_field_bb",
  "spot.line",
  "spot.board",
  "tree.script",
  "tree.params",
  "solver.storage",
  "solver.cfr_precision",
  "solver.algorithm.schedule",
  "solver.stop.target",
  "solver.stop.max_iterations",
  "solver.stop.check_every",
  "run.threads",
  "run.memory",
  "run.max_time",
  "run.checkpoint_interval",
  "run.final_checkpoint",
  "output.solution_streets",
]);

function leafPaths(table: Doc, prefix = ""): string[] {
  return Object.entries(table).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    if (prefix === "ranges") return [];
    if (path === "profile") return [];
    return typeof value === "object" && value !== null && !Array.isArray(value)
      ? leafPaths(value as Doc, path)
      : [path];
  });
}

/**
 * Where to fix a validation issue: the step with a field for the key it
 * names, else TOML. `onField` when that field shows the reason under itself.
 * The name sits over the table on every step, so it leads nowhere.
 */
function issueStep(issue: Issue): { step: Step; onField: boolean } | undefined {
  if (issue.path === "profile" || issue.path.startsWith("profile.") || issue.path.startsWith("profile[")) {
    return { step: "profile", onField: false };
  }
  if (issue.path === "meta.name") return undefined;
  const step = KEY_STEP[issue.path.split(".")[0]!];
  if (step === undefined) return undefined;
  const onField =
    step !== "toml" &&
    (issue.path.startsWith("ranges.") ||
      issue.path.startsWith("table.stacks_bb.") ||
      [...FORM_KEYS].some((key) => issue.path === key || issue.path.startsWith(`${key}.`)));
  const below = [...FORM_KEYS].some((key) => key.startsWith(`${issue.path}.`));
  return { step: onField || below ? step : "toml", onField };
}

/** A document with no name, so the name follows the spot (the placeholder). */
const unnamed = (doc: Doc): Doc => setAt(doc, ["meta", "name"], undefined);

function initialDoc(): Doc {
  const saved = localStorage.getItem(DRAFT_KEY);
  if (saved) {
    try {
      return readDoc(saved);
    } catch {
      // A draft from an older GUI that no longer parses: start from the template.
    }
  }
  return unnamed(readDoc(TEMPLATES.find((t) => t.name === DEFAULT_TEMPLATE)?.text ?? TEMPLATES[0]!.text));
}

/**
 * The spot's name as a run id and file name: letters, digits, `.`, `_` and
 * `-` stay; any other run (spaces, path separators) becomes one `_`. The
 * daemon reads a run id from the URL path as written, so it must not need
 * percent-encoding.
 */
function fileName(text: string): string {
  return text
    .trim()
    .replace(/[^\w.-]+/g, "_")
    .replace(/^[_.-]+|[_.-]+$/g, "");
}

/**
 * The name a spot gets when it has none: the table size, the two players
 * (IP first, as "BTN vs BB" reads) and the board, all from the line replay.
 */
function spotName(cursor: LineCursor | null, roles: Record<SizeRole, string> | null, board: string[]): string {
  if (!cursor) return "spot";
  const size = `${cursor.seats.length}max`;
  if (!roles) return `${size} ${cursor.street}`;
  return [size, `${roles.ip} vs ${roles.oop}`, board.join("")].filter(Boolean).join(" ");
}

/**
 * The daemon's replay (`POST /v1/line`) of the document's `[table]` and
 * `spot.line`, or of `draft`, the explicit moves the line was built from
 * while it is still a prefix the strict line rules read otherwise. A line
 * the table does not allow falls back to the Preflop root; the line field
 * shows why (validation names `spot.line`).
 */
function useLineCursor(toml: string, tableKey: string, line: string, draft: string[] | null) {
  const [cursor, setCursor] = useState<LineCursor | null>(null);
  const sequence = useRef(0);
  useEffect(() => {
    const timer = setTimeout(async () => {
      const id = ++sequence.current;
      for (const moves of [draft ?? undefined, []]) {
        try {
          const next = await api.line(toml, moves);
          if (id === sequence.current) setCursor(next);
          return;
        } catch {
          // Try the next replay; with none left the table is not drawn.
        }
      }
      if (id === sequence.current) setCursor(null);
    }, 250);
    return () => clearTimeout(timer);
    // The line API reads only `[table]` and `spot.line`.
  }, [tableKey, line, draft?.join(",")]);
  return cursor;
}

/** OOP and IP once the line starts a postflop street with two players left: OOP acts first. */
function cursorRoles(cursor: LineCursor | null): Record<SizeRole, string> | null {
  const oop = cursor?.complete ? cursor.next?.position : undefined;
  const left = cursor?.seats.filter((seat) => seat.status !== "folded").map((seat) => seat.position) ?? [];
  const ip = left.find((position) => position !== oop);
  return oop && ip && left.length === 2 ? { oop, ip } : null;
}

/**
 * `[ranges]` keyed by the two players the line leaves in. A position that
 * stays keeps its range; a new one takes the range it had before in this
 * document, else the range of the player who held its role, else the only
 * range that left. Returns null when the keys already fit; `note` says what
 * changed.
 */
function followRanges(
  doc: Doc,
  roles: Record<SizeRole, string>,
  previous: Record<SizeRole, string> | null,
  memory: Map<string, string>,
): { doc: Doc; note: string } | null {
  const ranges = (getAt(doc, ["ranges"]) as Doc | undefined) ?? {};
  const keys = Object.keys(ranges);
  const pair = [roles.oop, roles.ip];
  const leaving = keys.filter((key) => !pair.includes(key));
  if (leaving.length === 0) return null;
  const free = new Map(leaving.map((key) => [key, fieldText(ranges[key])]));
  for (const [key, text] of free) memory.set(key, text);
  let next = leaving.reduce((d, key) => setAt(d, ["ranges", key], undefined), doc);
  const moved: string[] = [];
  const restored: string[] = [];
  for (const role of ["oop", "ip"] as const) {
    const position = roles[role];
    if (keys.includes(position)) continue;
    const remembered = memory.get(position);
    if (remembered !== undefined) {
      next = setAt(next, ["ranges", position], remembered);
      restored.push(position);
      continue;
    }
    const source =
      previous && free.has(previous[role]) ? previous[role] : free.size === 1 ? [...free.keys()][0] : undefined;
    if (source === undefined) continue;
    next = setAt(next, ["ranges", position], free.get(source));
    moved.push(`${source}→${position}`);
    free.delete(source);
  }
  const note = [
    moved.length > 0 ? `Moved ${moved.join(", ")}; adjust each moved range to its new seat.` : "",
    restored.length > 0 ? `${restored.join(", ")} got back the range used before.` : "",
    free.size > 0 ? `Removed the range of ${[...free.keys()].join(", ")} (it returns if that seat does).` : "",
  ]
    .filter(Boolean)
    .join(" ");
  return { doc: next, note };
}

export function SetupPage() {
  const [doc, setDoc] = useState<Doc>(initialDoc);
  const [step, setStep] = useState<Step>("table");
  const [profilePrefill, setProfilePrefill] = useState<{ node: string; combos?: Record<string, Record<string, number>> } | null>(null);
  useEffect(() => {
    const text = sessionStorage.getItem("solvers.setup.profilePrefill");
    if (!text) return;
    sessionStorage.removeItem("solvers.setup.profilePrefill");
    try {
      const prefill = JSON.parse(text) as { toml: string; node: string; combos?: Record<string, Record<string, number>> };
      if (typeof prefill.toml !== "string" || typeof prefill.node !== "string") return;
      const loaded = readDoc(prefill.toml);
      setDoc(loaded);
      setProfilePrefill({ node: prefill.node, combos: prefill.combos });
      setStep("profile");
    } catch {
      // Keep the existing draft if the transferred config cannot be read.
    }
  }, []);
  const [templateDialog, setTemplateDialog] = useState(false);
  const [boardDialog, setBoardDialog] = useState(false);
  const [validation, setValidation] = useState<Validation>({
    state: "pending",
  });
  // Keep parameter controls available while validation is pending or an override is invalid.
  const [treeParams, setTreeParams] = useState<{ script: string; params: TreeParam[] } | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [tomlText, setTomlText] = useState<string | null>(null);
  const [tomlError, setTomlError] = useState<Error | null>(null);
  const [picked, setPicked] = useState<string[] | null>(null);
  const [rangeNote, setRangeNote] = useState<string | null>(null);
  // The last settled validation's key-bound error; a pending validation keeps it, so fields do not flicker.
  const [issue, setIssue] = useState<Issue | null>(null);
  // The moves the line was last built from by picking actions, with the line they spelled.
  const [draft, setDraft] = useState<{ moves: string[]; line: string } | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const body = useRef<HTMLElement>(null);
  const sequence = useRef(0);
  const rangeMemory = useRef(new Map<string, string>());
  const followed = useRef<Record<SizeRole, string> | null>(null);

  const toml = useMemo(() => writeDoc(doc), [doc]);

  const validate = async (text: string) => {
    const id = ++sequence.current;
    setValidation({ state: "pending" });
    try {
      const response = await api.validate(text, true);
      if (id !== sequence.current) return;
      setValidation({ state: "ok", response });
      setTreeParams({
        script: fieldText(getAt(readDoc(text), ["tree", "script"])),
        params: response.summary.tree?.params ?? [],
      });
      setIssue(null);
    } catch (error) {
      if (id !== sequence.current) return;
      const message = (error as Error).message;
      setValidation({ state: "error", message });
      setIssue(parseIssue(message));
    }
  };

  useEffect(() => {
    localStorage.setItem(DRAFT_KEY, toml);
  }, [toml]);

  // The TOML step keeps the user's own text (comments, order) while it still
  // reads as the document; any other change of the document rewrites it.
  useEffect(() => {
    if (tomlText === null) return;
    try {
      if (writeDoc(readDoc(tomlText)) === toml) return;
    } catch {
      // Unreadable text cannot be the document; the document changed elsewhere.
    }
    setTomlText(null);
    setTomlError(null);
  }, [toml]);

  // A new step starts at the top.
  useEffect(() => {
    body.current?.scrollTo({ top: 0 });
  }, [step]);

  const line = fieldText(getAt(doc, ["spot", "line"]));
  // The built moves stand for the line until the line changes some other way.
  const draftMoves = draft && draft.line === line ? draft.moves : null;
  const cursor = useLineCursor(toml, JSON.stringify(getAt(doc, ["table"]) ?? null), line, draftMoves);
  const boardText = fieldText(getAt(doc, ["spot", "board"]));
  const fullBoard = useMemo(() => parseBoard(boardText) ?? [], [boardText]);
  // Cards past those the line deals are kept in the document, for a longer
  // line to use, but left out of the spot: they are shown dimmed, and
  // validation, Solve and the saved TOML take the board without them.
  const dealt = cursor?.complete && cursor.line === line ? cursor.boardCards : null;
  const board = useMemo(
    () => (dealt !== null && fullBoard.length > dealt ? fullBoard.slice(0, dealt) : fullBoard),
    [fullBoard, dealt],
  );
  const ignored = fullBoard.slice(board.length);

  // Active players: the seats pressed, else the two the line leaves in. Each
  // replay that leaves two players in (a preset, an edit, a template) resets the picks.
  const remaining = cursor?.seats.filter((seat) => seat.status !== "folded").map((seat) => seat.position) ?? [];
  useEffect(() => {
    if (remaining.length === 2) setPicked(null);
  }, [cursor]);
  const selected = picked ?? (remaining.length === 2 ? remaining : []);
  const toggle = (position: string) =>
    setPicked(selected.includes(position) ? selected.filter((p) => p !== position) : [...selected, position].slice(-2));

  // Ranges follow the two players the line leaves in, except while the TOML
  // step edits the document as written.
  const lineRoles = cursorRoles(cursor);
  const rolesKey = lineRoles ? `${lineRoles.oop},${lineRoles.ip}` : "";
  const editingToml = step === "toml";
  useEffect(() => {
    if (!lineRoles || editingToml) return;
    const result = followRanges(doc, lineRoles, followed.current, rangeMemory.current);
    followed.current = lineRoles;
    if (result) {
      setDoc(result.doc);
      setRangeNote(result.note);
    }
  }, [rolesKey, editingToml]);

  // One name for the spot, its run directory and the saved TOML. Empty
  // follows the spot (the placeholder) and is written into what is sent.
  const writtenName = fieldText(getAt(doc, ["meta", "name"]));
  const autoName = spotName(cursor, lineRoles, board);
  const name = writtenName.trim() || autoName;
  const spotToml = useMemo(() => {
    let sent = ignored.length > 0 ? setAt(doc, ["spot", "board"], board.join(" ")) : doc;
    if (!writtenName.trim()) sent = setAt(sent, ["meta", "name"], autoName);
    return sent === doc ? toml : writeDoc(sent);
  }, [toml, board, ignored.length, writtenName, autoName]);

  // Validate shortly after the last edit; the daemon answers from the CLI.
  useEffect(() => {
    const timer = setTimeout(() => void validate(spotToml), 700);
    return () => clearTimeout(timer);
  }, [spotToml]);

  const summary = validation.state === "ok" ? validation.response.summary : null;
  const start = summary?.start;
  const roles = lineRoles ?? (start ? { oop: start.oop.position, ip: start.ip.position } : null);
  const routed = issue ? issueStep(issue) : undefined;
  const errorStep = routed && [...STEPS, TOML_STEP].find((s) => s.id === routed.step);
  const index = STEPS.findIndex((s) => s.id === step);
  const nextStep = STEPS[index + 1];
  // A reason no field shows goes to the top of its step.
  const stepIssue = routed && !routed.onField && routed.step === step ? issue : null;

  // TOML is entered from a step and left back to it.
  const stepBeforeToml = useRef<Step>("table");
  const openStep = (id: Step) => {
    if (id === "toml" && step !== "toml") stepBeforeToml.current = step;
    setStep(id);
  };

  // A new document starts its range history afresh.
  const replace = (next: Doc) => {
    setTreeParams(null);
    setDoc(next);
    setRangeNote(null);
    rangeMemory.current.clear();
  };

  // The run directory is the name; a name another run already has gets a number after it.
  const solve = async () => {
    setSubmitting(true);
    setSubmitError(null);
    try {
      const base = fileName(name) || "spot";
      const taken = new Set((await api.runs()).map((run) => run.runId));
      let runId = base;
      for (let n = 2; taken.has(runId); n++) runId = `${base}-${n}`;
      const created = await api.createRun(spotToml, runId);
      navigate({ page: "runs", runId: created.runId });
    } catch (error) {
      setSubmitError((error as Error).message);
    } finally {
      setSubmitting(false);
    }
  };
  const canSolve = !submitting && validation.state === "ok" && tomlError === null;

  const openFile = async (file: File) => {
    try {
      replace(readDoc(await file.text()));
    } catch (error) {
      setValidation({
        state: "error",
        message: `${file.name}: ${(error as Error).message}`,
      });
    }
  };

  // A TOML edit that parses becomes the document; one that does not is shown
  // with its error and leaves the document as it was.
  const editToml = (value: string) => {
    setTomlText(value);
    try {
      setDoc(readDoc(value));
      setTomlError(null);
    } catch (error) {
      setTomlError(error as Error);
    }
  };

  return (
    <DocContext.Provider value={{ doc, setDoc, issue }}>
      <header className="page-header">
        <h1>Setup</h1>
        <Badge variant="primary" icon="cards">
          HU Postflop
        </Badge>
        <span className="grow" />
        <input
          ref={fileInput}
          type="file"
          accept=".toml"
          hidden
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void openFile(file);
            event.target.value = "";
          }}
        />
        <Button variant="outline" icon="file-import" onClick={() => fileInput.current?.click()}>
          Open TOML
        </Button>
        <Button
          variant="outline"
          icon="device-floppy"
          onClick={() => saveBlob(new Blob([spotToml], { type: "text/plain" }), `${fileName(name) || "spot"}.toml`)}
        >
          Save TOML
        </Button>
        <Button icon="player-play" disabled={!canSolve} onClick={solve}>
          Solve
        </Button>
      </header>
      <main ref={body} className="page-body setup-body">
        <div className="setup-form">
          <div className="setup-steps">
            <Tabs value={step} onValueChange={(id) => setStep(STEPS.find((s) => s.id === id)?.id ?? "table")}>
              <TabsList>
                {STEPS.map((s) => (
                  <TabsTrigger
                    key={s.id}
                    value={s.id}
                    icon={s.icon}
                    title={s.id === errorStep?.id ? "Validation found a problem in this step" : undefined}
                  >
                    {s.label}
                    {s.id === errorStep?.id && <Icon name="alert-circle" size={14} className="step-alert" />}
                  </TabsTrigger>
                ))}
              </TabsList>
            </Tabs>
            <span className="grow" />
            <Button
              variant="outline"
              size="sm"
              icon="template"
              aria-haspopup="dialog"
              onClick={() => setTemplateDialog(true)}
            >
              Load template
            </Button>
            <span className="setup-steps-divider" aria-hidden />
            <Button
              variant={step === "toml" ? "secondary" : "outline"}
              size="sm"
              icon={TOML_STEP.icon}
              aria-pressed={step === "toml"}
              title={step === "toml" ? "Back to the steps" : "Edit the whole document as TOML"}
              onClick={() => openStep(step === "toml" ? stepBeforeToml.current : "toml")}
            >
              Edit TOML
              {errorStep?.id === "toml" && <Icon name="alert-circle" size={14} className="step-alert" />}
            </Button>
          </div>
          {submitError && <Notice variant="destructive" title="Cannot create the run" description={submitError} />}
          {stepIssue && (
            <Notice
              variant="destructive"
              title="Validation found a problem"
              description={`${stepIssue.path}: ${stepIssue.message}`}
            />
          )}

          <section className="setup-step" hidden={step !== "table"}>
            <SetupTable />
          </section>
          <section className="setup-step" hidden={step !== "start"}>
            <SetupStart
              toml={toml}
              cursor={cursor}
              building={draftMoves !== null}
              selected={selected}
              board={board}
              ignored={ignored}
              onBoardDialog={() => setBoardDialog(true)}
              onBuilt={(built) => {
                setDraft({ moves: built.moves, line: built.line });
                setDoc((d) => setAt(d, ["spot", "line"], built.line || undefined));
              }}
            />
          </section>
          <section className="setup-step" hidden={step !== "range"}>
            <RangeStep roles={roles} board={board} note={rangeNote} />
          </section>
          <section className="setup-step" hidden={step !== "tree"}>
            <SetupTree
              startStreet={start?.street ?? (lineRoles ? (cursor?.street ?? null) : null)}
              roles={roles}
              params={treeParams?.script === fieldText(getAt(doc, ["tree", "script"])) ? treeParams.params : []}
            />
          </section>
          <section className="setup-step" hidden={step !== "profile"}>
            <SetupProfile prefill={profilePrefill} />
          </section>
          <section className="setup-step" hidden={step !== "solver"}>
            <SolverStep />
          </section>
          {step === "toml" && (
            <TomlStep
              text={tomlText ?? toml}
              error={tomlError}
              unmodeled={leafPaths(doc).filter(
                (path) => !FORM_KEYS.has(path) && !path.startsWith("table.stacks_bb.") && !path.startsWith("tree.params."),
              )}
              onChange={editToml}
            />
          )}

          {step === "solver" ? (
            <div className="step-footer">
              <Button icon="player-play" disabled={!canSolve} onClick={solve}>
                Solve
              </Button>
            </div>
          ) : (
            step !== "toml" &&
            nextStep && (
              <div className="step-footer">
                <Button variant="outline" iconRight="arrow-right" onClick={() => setStep(nextStep.id)}>
                  Next: {nextStep.label}
                </Button>
              </div>
            )
          )}
        </div>

        <aside className="setup-aside">
          <SpotCard
            cursor={cursor}
            board={board}
            ignored={ignored}
            roles={lineRoles}
            name={writtenName}
            autoName={autoName}
            nameError={issue?.path === "meta.name" ? issue.message : undefined}
            selected={selected}
            onName={(value) => setDoc((d) => setAt(d, ["meta", "name"], value || undefined))}
            onToggle={toggle}
            onBoardDialog={() => setBoardDialog(true)}
          />
          <ValidationCard
            validation={validation}
            storageAuto={!getAt(doc, ["solver", "storage"]) || getAt(doc, ["solver", "storage"]) === "auto"}
            errorStep={
              errorStep && errorStep.id !== step ? { label: errorStep.label, open: () => openStep(errorStep.id) } : null
            }
            onRevalidate={() => void validate(spotToml)}
          />
          <TreeCard toml={spotToml} ready={validation.state === "ok"} roles={lineRoles} start={start ?? null} />
        </aside>
      </main>

      {templateDialog && (
        <TemplateDialog
          onClose={() => setTemplateDialog(false)}
          onPick={(next) => {
            replace(unnamed(next));
            setTemplateDialog(false);
          }}
        />
      )}
      {boardDialog && (
        <BoardDialog
          initial={fullBoard}
          onClose={() => setBoardDialog(false)}
          onApply={(cards) => {
            setDoc((d) => setAt(d, ["spot", "board"], cards.length > 0 ? cards.join(" ") : undefined));
            setBoardDialog(false);
          }}
        />
      )}
    </DocContext.Provider>
  );
}

/** The Range step: one range per remaining player, with OOP and IP swappable (GTO Wizard's ⇄). */
function RangeStep({
  roles,
  board,
  note,
}: {
  roles: Record<SizeRole, string> | null;
  board: string[];
  /** What the last change of players did to the ranges, to say so. */
  note: string | null;
}) {
  const { doc, setDoc, update } = useDoc();
  const issue = useIssue("ranges");
  const issueSeat = issue?.path.slice("ranges.".length);
  const [chosen, setChosen] = useState<string | null>(null);
  const ranges = (getAt(doc, ["ranges"]) as Doc | undefined) ?? {};
  const keys = Object.keys(ranges);
  const seats = roles ? [roles.oop, roles.ip] : keys;
  const extra = keys.filter((key) => !seats.includes(key));
  const active = chosen && [...seats, ...extra].includes(chosen) ? chosen : (seats[0] ?? null);
  const label = (position: string) =>
    roles?.oop === position ? `${position}（OOP）` : roles?.ip === position ? `${position}（IP）` : position;
  const swap = (pair: Record<SizeRole, string>) =>
    setDoc((d) => {
      const oop = fieldText(getAt(d, ["ranges", pair.oop]));
      const ip = fieldText(getAt(d, ["ranges", pair.ip]));
      return setAt(setAt(d, ["ranges", pair.oop], ip || undefined), ["ranges", pair.ip], oop || undefined);
    });

  return (
    <Card>
      <CardHeader>
        <CardTitle>Range</CardTitle>
        {roles && (
          <CardAction>
            <Button variant="outline" size="sm" icon="arrows-exchange" onClick={() => swap(roles)}>
              Swap OOP and IP
            </Button>
          </CardAction>
        )}
      </CardHeader>
      <CardContent>
        <div className="stack" style={{ gap: 16 }}>
          {note && <Notice variant="info" title="Ranges moved to the two players left" description={note} />}
          {active ? (
            <RangeEditor
              text={fieldText(ranges[active])}
              board={board}
              toolbar={
                <Tabs
                  variant="pill"
                  items={[...seats, ...extra].map((position) => ({
                    id: position,
                    label: label(position),
                    icon: extra.includes(position) || position === issueSeat ? "alert-triangle" : undefined,
                  }))}
                  value={active}
                  onValueChange={setChosen}
                />
              }
              issue={
                issue && (issueSeat && issueSeat !== active ? `${issueSeat} range: ${issue.message}` : issue.message)
              }
              onChange={(value) => update(["ranges", active], value.trim() ? value : undefined)}
            />
          ) : (
            <EmptyState
              size="sm"
              icon="cards"
              title="Once the line starts a postflop street, edit the ranges of the two players left"
            />
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function SolverStep() {
  const { doc, update } = useDoc();
  return (
    <>
      <Card title="Stopping">
        <div className="form-grid">
          <DocText
            label="Target"
            path={["solver", "stop", "target"]}
            placeholder="none"
            hint="Target for NashConv / 2. Cash: 0.1%pot / 0.05bb; ICM: 0.01%prizes"
          />
          <DocText label="Max iterations" path={["solver", "stop", "max_iterations"]} placeholder="1000000" />
          <DocText
            label="Check every"
            path={["solver", "stop", "check_every"]}
            placeholder="auto"
            hint="auto checks every 3–50 iterations (adaptive) with a target, every 25 without. An integer fixes the interval"
          />
          <DocText label="Max time" path={["run", "max_time"]} placeholder="none" hint="Total solve time. s / m / h" />
        </div>
      </Card>
      <Card title="Resources and output">
        <div className="stack" style={{ gap: 16 }}>
          <div className="form-grid">
            <DocText label="Threads" path={["run", "threads"]} placeholder="auto" />
            <DocText
              label="Memory limit"
              path={["run", "memory"]}
              placeholder="auto"
              hint="Bytes as an integer, or an integer with KiB/MiB/GiB. auto is 80% of physical RAM"
            />
            <DocSelect
              label="Saved streets"
              path={["output", "solution_streets"]}
              options={["full", "no-rivers"]}
              defaultLabel="Default (full)"
            />
            <DocText label="Checkpoint interval" path={["run", "checkpoint_interval"]} placeholder="15m" />
          </div>
          <Checkbox
            label="Checkpoint at the end"
            checked={getAt(doc, ["run", "final_checkpoint"]) !== false}
            onCheckedChange={(checked) => update(["run", "final_checkpoint"], checked === true ? undefined : false)}
          />
          <FieldIssue keys={["run.final_checkpoint"]} />
        </div>
      </Card>
      <Card title="Algorithm">
        <div className="form-grid">
          <DocSelect
            label="Schedule"
            path={["solver", "algorithm", "schedule"]}
            options={["dcfr", "hs-dcfr", "linear-cfr", "cfr-plus", "vanilla"]}
            defaultLabel="Default (dcfr)"
          />
          <DocSelect
            label="CFR precision"
            path={["solver", "cfr_precision"]}
            options={["f32", "f64"]}
            defaultLabel="Default (f32)"
          />
          <DocSelect
            label="Storage"
            path={["solver", "storage"]}
            options={["auto", "f32", "i16-f32avg", "i16"]}
            defaultLabel="Default (auto)"
            hint="auto picks f32 if it fits the memory limit, else i16-f32avg"
          />
        </div>
      </Card>
    </>
  );
}

/** The whole document as TOML; text that does not parse stays here and leaves the document as it was. */
function TomlStep({
  text,
  error,
  unmodeled,
  onChange,
}: {
  text: string;
  error: Error | null;
  /** Keys no step has a field for; they are kept as written. */
  unmodeled: string[];
  onChange: (text: string) => void;
}) {
  const { issue } = useDoc();
  // A parse error marks its own position; otherwise the daemon's error marks the key (or script line) it names.
  const diagnostics = useMemo((): LineDiagnostic[] => {
    if (error instanceof TomlError) {
      return [{ line: error.line, column: error.column, message: error.message.split("\n")[0]!, severity: "error" }];
    }
    if (error || !issue) return [];
    const line = tomlIssueLine(text, issue.path, issue.message);
    return line === null ? [] : [{ line, message: `${issue.path}: ${issue.message}`, severity: "error" }];
  }, [text, error, issue]);
  return (
    <Card
      title="Edit TOML"
      description="Apart from the steps, write the whole document as TOML. Each edit that reads as TOML updates the steps, and step edits rewrite it here."
    >
      <div className="stack" style={{ gap: 12 }}>
        {error && (
          <Notice
            variant="destructive"
            title="Not readable as TOML"
            description={`The steps and validation keep the last readable document. ${error.message}`}
          />
        )}
        <CodeEditor className="toml-editor" label="TOML" language="toml" value={text} diagnostics={diagnostics} onChange={onChange} />
        {unmodeled.length > 0 && (
          <p className="muted meta" style={{ margin: 0 }}>
            Keys no step has a field for (kept as written): <span className="mono">{unmodeled.join(", ")}</span>
          </p>
        )}
      </div>
    </Card>
  );
}
