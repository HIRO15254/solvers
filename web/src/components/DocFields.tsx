import { createContext, useContext, useId, type ReactNode } from "react";
import type { TomlPrimitive } from "smol-toml";
import { Select } from "../cryst";
import { fieldText, fieldValue, getAt, listValue, setAt, type Doc, type Path } from "../lib/config";
import { DraftInput, Field } from "./common";

/** A validation error the daemon tied to one key: `NLH003: economics.rake.rate: must be …`. */
export interface Issue {
  /** The key path the error names, e.g. `ranges.BTN`. */
  path: string;
  /** The reason, without the code and the path. */
  message: string;
}

/** The key path and reason of a daemon error message, when it names one. */
export function parseIssue(message: string): Issue | null {
  const match = /NLH\d{3}: ([A-Za-z0-9_.]+): ([\s\S]*)/.exec(message);
  return match ? { path: match[1]!, message: match[2]!.trim() } : null;
}

/** The document the Setup steps edit, the one way to change it, and the last validation error. */
export interface DocEditor {
  doc: Doc;
  setDoc: (change: (doc: Doc) => Doc) => void;
  issue: Issue | null;
}

export const DocContext = createContext<DocEditor | null>(null);

/** The Setup document, with `update` to set (or, with `undefined`, remove) one key. */
export function useDoc() {
  const editor = useContext(DocContext);
  if (!editor) throw new Error("useDoc needs a DocContext provider");
  const update = (path: Path, value: TomlPrimitive | undefined) => editor.setDoc((d) => setAt(d, path, value));
  return { ...editor, update };
}

/** The validation error when it names one of `keys` (dotted paths) or a key below one. */
export function useIssue(...keys: string[]): Issue | undefined {
  const { issue } = useDoc();
  return issue && keys.some((key) => issue.path === key || issue.path.startsWith(`${key}.`)) ? issue : undefined;
}

/** The validator's reason under a control that has no error slot of its own. */
export function FieldIssue({ keys }: { keys: string[] }) {
  const issue = useIssue(...keys);
  return issue ? (
    <span className="cr-hint cr-hint-error" role="alert">
      {issue.message}
    </span>
  ) : null;
}

/**
 * A text field bound to the key at `path`; with `hint` the label gets an
 * InfoHint. `normalize` rewrites the typed text before the document reads it,
 * `list` reads a comma-separated list.
 */
export function DocText({
  label,
  path,
  placeholder = "",
  hint,
  mono = true,
  list = false,
  normalize,
}: {
  label: string;
  path: Path;
  placeholder?: string;
  hint?: ReactNode;
  mono?: boolean;
  list?: boolean;
  normalize?: (text: string) => string;
}) {
  const { doc, update } = useDoc();
  const issue = useIssue(path.join("."));
  const id = useId();
  const value = (typed: string) => {
    const written = normalize ? normalize(typed) : typed;
    return list ? listValue(written) : fieldValue(written);
  };
  const control = (
    <DraftInput
      id={id}
      label={hint ? undefined : label}
      className={mono ? "mono" : undefined}
      value={fieldText(getAt(doc, path))}
      read={(typed) => fieldText(value(typed))}
      placeholder={placeholder}
      spellCheck={false}
      onText={(typed) => update(path, value(typed))}
      issue={issue?.message}
    />
  );
  return hint ? (
    <Field label={label} hint={hint} htmlFor={id}>
      {control}
    </Field>
  ) : (
    control
  );
}

/** A choice bound to the key at `path`; the first option removes the key (the default). */
export function DocSelect({
  label,
  path,
  options,
  defaultLabel,
  hint,
}: {
  label: string;
  path: Path;
  options: string[];
  defaultLabel: string;
  hint?: ReactNode;
}) {
  const { doc, update } = useDoc();
  const id = useId();
  const issue = useIssue(path.join("."));
  const select = (
    <Select
      id={id}
      label={hint ? undefined : label}
      className="mono"
      aria-invalid={issue ? true : undefined}
      options={[{ value: "", label: defaultLabel }, ...options]}
      value={fieldText(getAt(doc, path))}
      onValueChange={(value) => update(path, value || undefined)}
    />
  );
  const error = issue && (
    <span className="cr-hint cr-hint-error" role="alert">
      {issue.message}
    </span>
  );
  if (hint)
    return (
      <Field label={label} hint={hint} htmlFor={id}>
        {select}
        {error}
      </Field>
    );
  return error ? (
    <div className="field">
      {select}
      {error}
    </div>
  ) : (
    select
  );
}
