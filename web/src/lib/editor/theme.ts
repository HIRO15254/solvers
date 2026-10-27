// The editor's look from Cryst tokens, so it follows `data-theme`.

import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { actionTag } from "./languages";

const highlight = HighlightStyle.define([
  { tag: [t.keyword, t.heading], color: "var(--primary)" },
  { tag: actionTag, color: "var(--warning)" },
  { tag: [t.number, t.bool], color: "var(--warning)" },
  { tag: [t.variableName, t.propertyName], color: "var(--success)" },
  { tag: t.definition(t.variableName), color: "var(--success)", fontWeight: "600" },
  { tag: t.string, color: "var(--info)" },
  { tag: [t.lineComment, t.operator, t.punctuation, t.separator, t.brace, t.squareBracket, t.paren], color: "var(--muted-foreground)" },
  { tag: t.invalid, color: "var(--destructive)" },
]);

const theme = EditorView.theme({
  "&": {
    color: "var(--foreground)",
    backgroundColor: "var(--card)",
    fontSize: "var(--text-sm)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--font-mono)",
    lineHeight: "var(--leading-normal)",
    fontVariantLigatures: "none",
  },
  ".cm-content": { padding: "var(--space-2) 0", caretColor: "var(--foreground)" },
  ".cm-line": { padding: "0 10px 0 6px" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--foreground)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "var(--selection)",
  },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--accent) 60%, transparent)" },
  ".cm-gutters": {
    backgroundColor: "var(--card)",
    color: "var(--muted-foreground)",
    border: "none",
    borderRight: "1px solid var(--border)",
  },
  ".cm-activeLineGutter": { backgroundColor: "var(--accent)", color: "var(--foreground)" },
  ".cm-lineNumbers .cm-gutterElement": { padding: "0 6px 0 10px", minWidth: "2ch" },
  ".cm-foldGutter .cm-gutterElement": { padding: "0 4px", cursor: "pointer" },
  ".cm-foldPlaceholder": {
    backgroundColor: "var(--secondary)",
    border: "1px solid var(--border)",
    color: "var(--muted-foreground)",
    padding: "0 4px",
  },
  "&.cm-focused .cm-matchingBracket": {
    backgroundColor: "var(--selection)",
    outline: "1px solid var(--ring)",
  },
  "&.cm-focused .cm-nonmatchingBracket": { color: "var(--destructive)", outline: "1px solid var(--destructive)" },
  ".cm-selectionMatch": { backgroundColor: "color-mix(in srgb, var(--primary) 18%, transparent)" },
  ".cm-searchMatch": {
    backgroundColor: "color-mix(in srgb, var(--warning) 25%, transparent)",
    outline: "1px solid color-mix(in srgb, var(--warning) 60%, transparent)",
  },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "color-mix(in srgb, var(--warning) 45%, transparent)" },
  ".cm-panels": {
    backgroundColor: "var(--popover)",
    color: "var(--popover-foreground)",
    fontFamily: "var(--font-sans)",
    fontSize: "var(--text-sm)",
  },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--border)" },
  ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--border)" },
  ".cm-panel.cm-search": { padding: "6px 28px 6px 8px" },
  ".cm-panel input, .cm-panel button, .cm-textfield, .cm-button": {
    fontFamily: "inherit",
    fontSize: "var(--text-sm)",
    color: "var(--foreground)",
    borderRadius: "var(--radius-sm)",
  },
  ".cm-textfield": {
    backgroundColor: "var(--background)",
    border: "1px solid var(--input)",
    padding: "2px 6px",
  },
  ".cm-textfield:focus": { outline: "none", borderColor: "var(--ring)" },
  ".cm-button": {
    backgroundImage: "none",
    backgroundColor: "var(--secondary)",
    border: "1px solid var(--border)",
    padding: "2px 8px",
  },
  ".cm-button:active": { backgroundImage: "none", backgroundColor: "var(--accent)" },
  ".cm-panel.cm-search label": { color: "var(--muted-foreground)" },
  // Tabler's x replaces the "×" character of the search and lint panels.
  ".cm-panels .cm-panel button[name=close]": { color: "var(--muted-foreground)", fontSize: "0", padding: "4px", cursor: "pointer" },
  ".cm-panels .cm-panel button[name=close]::before": { fontFamily: "tabler-icons", fontSize: "14px", content: '"\\eb55"' },
  ".cm-tooltip": {
    backgroundColor: "var(--popover)",
    color: "var(--popover-foreground)",
    border: "1px solid var(--border)",
    borderRadius: "var(--radius-md)",
    boxShadow: "var(--shadow-md)",
    fontSize: "var(--text-sm)",
  },
  ".cm-tooltip.cm-tooltip-autocomplete > ul": { fontFamily: "var(--font-mono)", maxHeight: "16em" },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li": { padding: "2px 8px" },
  ".cm-tooltip-autocomplete ul li[aria-selected]": { backgroundColor: "var(--selection)", color: "var(--foreground)" },
  ".cm-completionDetail": { color: "var(--muted-foreground)", fontStyle: "normal", marginLeft: "1em" },
  ".cm-completionMatchedText": { textDecoration: "none", color: "var(--primary)", fontWeight: "600" },
  ".cm-completionInfo": { padding: "6px 8px", maxWidth: "28em", fontFamily: "var(--font-sans)" },
  ".cm-doc-tooltip": { padding: "6px 8px", maxWidth: "32em", fontFamily: "var(--font-sans)", lineHeight: "var(--leading-normal)" },
  ".cm-doc-tooltip code": { fontFamily: "var(--font-mono)", color: "var(--primary)" },
  ".cm-diagnostic": { padding: "4px 8px", fontFamily: "var(--font-sans)" },
  ".cm-diagnostic-error": { borderLeft: "3px solid var(--destructive)" },
  ".cm-diagnostic-warning": { borderLeft: "3px solid var(--warning)" },
  ".cm-lintRange-error": {
    backgroundImage: "none",
    textDecoration: "underline wavy var(--destructive)",
    textUnderlineOffset: "3px",
  },
  ".cm-lintRange-warning": {
    backgroundImage: "none",
    textDecoration: "underline wavy var(--warning)",
    textUnderlineOffset: "3px",
  },
  ".cm-lint-marker": { content: "normal", width: "0.55em", height: "0.55em", borderRadius: "50%", margin: "0 auto" },
  ".cm-lint-marker-error": { content: "normal", backgroundColor: "var(--destructive)" },
  ".cm-lint-marker-warning": { content: "normal", backgroundColor: "var(--warning)" },
});

export function editorTheme(): Extension {
  return [theme, syntaxHighlighting(highlight)];
}
