import { Button, Dialog } from "../cryst";
import { fieldText, getAt, readDoc, TEMPLATES, type Doc } from "../lib/config";
import { parseBoard } from "../lib/cards";
import { amountDisplay } from "../lib/format";
import { Board } from "./common";

/** Each shipped example with what it sets up, read once at load. */
const ENTRIES = TEMPLATES.map((template) => {
  const doc = readDoc(template.text);
  const text = (path: string[]) => fieldText(getAt(doc, path));
  return {
    ...template,
    doc,
    title: text(["meta", "name"]) || template.name,
    table: `${text(["table", "players"])} players · ${amountDisplay(getAt(doc, ["table", "stack"]), Number(getAt(doc, ["table", "bb_chips"]) ?? 1000), text(["table", "chip_rounding"]) || "nearest") || "?"} · ${text(["economics", "kind"]) || "cash"}`,
    line: text(["spot", "line"]) || "Preflop root",
    board: parseBoard(text(["spot", "board"])) ?? [],
  };
});

/** Replaces the document with one of `examples/hu-postflop/`. */
export function TemplateDialog({ onClose, onPick }: { onClose: () => void; onPick: (doc: Doc) => void }) {
  return (
    <Dialog
      open
      onOpenChange={(open) => !open && onClose()}
      title="Load template"
      description="Replace all current input with an example from examples/hu-postflop."
      icon="template"
      closeLabel="Close"
      className="template-dialog"
      footer={
        <Button variant="outline" onClick={onClose}>
          Cancel
        </Button>
      }
    >
      <div className="template-list">
        {ENTRIES.map((entry) => (
          <button key={entry.name} type="button" className="template-row" onClick={() => onPick(entry.doc)}>
            <span className="template-title">{entry.title}</span>
            <span className="muted mono meta">
              {entry.name} · {entry.table}
            </span>
            <span className="row" style={{ gap: 8 }}>
              <span className="mono meta">{entry.line}</span>
              <Board cards={entry.board} small />
            </span>
          </button>
        ))}
      </div>
    </Dialog>
  );
}
