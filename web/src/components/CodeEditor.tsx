import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, foldGutter, foldKeymap, indentOnInput } from "@codemirror/language";
import { lintGutter, lintKeymap, setDiagnostics } from "@codemirror/lint";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Annotation, EditorState, type Extension } from "@codemirror/state";
import {
  crosshairCursor,
  drawSelection,
  dropCursor,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import { useEffect, useRef } from "react";
import { scriptCompletionSource, scriptHover } from "../lib/editor/completion";
import { type LineDiagnostic, toDiagnostics } from "../lib/editor/diagnostics";
import { scriptSupport, tomlSupport } from "../lib/editor/languages";
import { editorTheme } from "../lib/editor/theme";

export type EditorLanguage = "script" | "toml";

/** Marks a transaction that mirrors the `value` prop, so it is not echoed to `onChange`. */
const fromProps = Annotation.define<boolean>();

function foldMarker(open: boolean): HTMLElement {
  const icon = document.createElement("i");
  icon.className = `ti ti-${open ? "chevron-down" : "chevron-right"}`;
  icon.setAttribute("aria-hidden", "true");
  return icon;
}

function extensions(language: EditorLanguage, label: string, id: string | undefined, onChange: (text: string) => void): Extension {
  const embedded = language === "toml";
  return [
    lineNumbers(),
    highlightActiveLineGutter(),
    foldGutter({ markerDOM: foldMarker }),
    lintGutter(),
    history(),
    drawSelection(),
    dropCursor(),
    EditorState.allowMultipleSelections.of(true),
    indentOnInput(),
    bracketMatching(),
    closeBrackets(),
    autocompletion({ override: [scriptCompletionSource(embedded)], icons: false }),
    rectangularSelection(),
    crosshairCursor(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    search({ top: true }),
    scriptHover(embedded),
    keymap.of([
      ...closeBracketsKeymap,
      ...defaultKeymap,
      ...searchKeymap,
      ...historyKeymap,
      ...foldKeymap,
      ...completionKeymap,
      ...lintKeymap,
      indentWithTab,
    ]),
    EditorView.lineWrapping,
    EditorView.contentAttributes.of({ "aria-label": label, ...(id ? { id } : {}) }),
    embedded ? tomlSupport() : scriptSupport(),
    editorTheme(),
    EditorView.updateListener.of((update) => {
      if (update.docChanged && !update.transactions.some((tr) => tr.annotation(fromProps))) {
        onChange(update.state.doc.toString());
      }
    }),
  ];
}

/** The smallest single replacement turning `before` into `after`, so the cursor outside it stays put. */
function difference(before: string, after: string): { from: number; to: number; insert: string } {
  let start = 0;
  const limit = Math.min(before.length, after.length);
  while (start < limit && before[start] === after[start]) start += 1;
  let end = 0;
  while (end < limit - start && before[before.length - 1 - end] === after[after.length - 1 - end]) end += 1;
  return { from: start, to: before.length - end, insert: after.slice(start, after.length - end) };
}

/**
 * A CodeMirror editor for the tree script or for TOML with an embedded
 * script: highlighting, completion, hover docs, folding, search and the
 * daemon's error on its line. The daemon remains the parser of record.
 */
export function CodeEditor({
  id,
  label,
  value,
  onChange,
  language,
  diagnostics,
  className,
}: {
  id?: string;
  /** The accessible name of the editing area. */
  label: string;
  value: string;
  onChange: (value: string) => void;
  language: EditorLanguage;
  /** Errors to mark, by 1-based line of `value`. */
  diagnostics?: readonly LineDiagnostic[];
  className?: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const changed = useRef(onChange);
  changed.current = onChange;

  useEffect(() => {
    const editor = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: extensions(language, label, id, (text) => changed.current(text)),
      }),
    });
    view.current = editor;
    return () => {
      editor.destroy();
      view.current = null;
    };
    // The editor is built once per language; `value` and `diagnostics` sync below.
  }, [language, label, id]);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    const current = editor.state.doc.toString();
    if (current !== value) {
      editor.dispatch({ changes: difference(current, value), annotations: fromProps.of(true) });
    }
    editor.dispatch(setDiagnostics(editor.state, toDiagnostics(editor.state.doc, diagnostics ?? [])));
  }, [value, diagnostics, language, label, id]);

  return <div ref={host} className={`code-editor ${className ?? ""}`} />;
}
