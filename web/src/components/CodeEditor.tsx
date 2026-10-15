import { Fragment, type ReactNode } from "react";

// Highlighting only colors tokens; the daemon parses and reports errors.

/** A transparent textarea over its highlighted copy; `highlight` returns one node list per line. */
export function CodeEditor({
  id,
  value,
  onChange,
  highlight,
  className,
}: {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  highlight: (text: string) => ReactNode[][];
  className?: string;
}) {
  return (
    <div className={`code-editor ${className ?? ""}`}>
      <pre aria-hidden="true">
        {highlight(value).map((line, i) => (
          <Fragment key={i}>
            {line}
            {"\n"}
          </Fragment>
        ))}
      </pre>
      <textarea
        id={id}
        spellCheck={false}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key !== "Tab") return;
          event.preventDefault();
          const target = event.currentTarget;
          const { selectionStart, selectionEnd } = target;
          onChange(value.slice(0, selectionStart) + "  " + value.slice(selectionEnd));
          requestAnimationFrame(() => target.setSelectionRange(selectionStart + 2, selectionStart + 2));
        }}
      />
    </div>
  );
}

const token = (key: number, className: string, text: string): ReactNode =>
  className ? (
    <span key={key} className={className}>
      {text}
    </span>
  ) : (
    text
  );

// The `[tree] script` dialect (docs/nlh-input-v1.jp.md §9).
const KEYWORD =
  /^(?:preflop|flop|turn|river|when|if|else|param|define|add|remove|replace|force|checkdown|in)$/;
const ACTION = /^(?:fold|check|call|bet|raise)$/;
const NUMBER = /^\d+(?:\.\d+)?(?:%|x|e|bb|%pot|%stack|%effective)?$|^a$/;
const SCRIPT_TOKEN = /(#.*$)|(\s+)|(&&|\|\||[<>=!]=?|[{}[\](),])|([^\s{}[\](),<>=!&|#]+)/g;

export function highlightScript(text: string): ReactNode[][] {
  return text.split("\n").map((line) =>
    Array.from(line.matchAll(SCRIPT_TOKEN), (match) => {
      const [text, comment, space, punct, word] = match;
      if (space) return text;
      const className = comment
        ? "tok-com"
        : punct
          ? "tok-punct"
          : word && KEYWORD.test(word)
            ? "tok-kw"
            : word && ACTION.test(word)
              ? "tok-act"
              : word && NUMBER.test(word)
                ? "tok-num"
                : "tok-var";
      return token(match.index, className, text);
    }),
  );
}

// TOML 1.0: table headers, keys, strings (multi-line ones span lines), numbers,
// booleans and dates, comments. Value rules are tried in order at each position.
const TOML_HEADER = /^\s*\[\[?[^\]]*\]\]?/;
const TOML_PART = String.raw`(?:[A-Za-z0-9_-]+|"(?:[^"\\]|\\.)*"|'[^']*')`;
const TOML_KEY = new RegExp(String.raw`^\s*${TOML_PART}(?:\s*\.\s*${TOML_PART})*(?=\s*=)`);
const TOML_RULES: [RegExp, string][] = [
  [/#.*/y, "tok-com"],
  [/"""|'''/y, "open"],
  [/"(?:[^"\\]|\\.)*"?|'[^']*'?/y, "tok-str"],
  [/\d{4}-\d{2}-\d{2}(?:[Tt ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:[Zz]|[+-]\d{2}:\d{2})?)?/y, "tok-num"],
  [/[+-]?(?:inf|nan|0x[\da-fA-F_]+|0o[0-7_]+|0b[01_]+|\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?)(?![\w-])/y, "tok-num"],
  [/(?:true|false)(?![\w-])/y, "tok-num"],
  [/[A-Za-z0-9_-]+/y, "word"],
  [/[=[\]{},.]/y, "tok-punct"],
  [/\s+/y, ""],
  [/./y, ""],
];

export function highlightToml(text: string): ReactNode[][] {
  let open: string | null = null;
  return text.split("\n").map((line) => {
    const out: ReactNode[] = [];
    let at = 0;
    // A table header, or the (possibly dotted) key of a `key = value` line.
    const lead = open === null ? (TOML_HEADER.exec(line) ?? TOML_KEY.exec(line)) : null;
    if (lead) {
      out.push(token(0, lead[0].trimStart().startsWith("[") ? "tok-kw" : "tok-var", lead[0]));
      at = lead[0].length;
    }
    while (at < line.length) {
      if (open !== null) {
        const end = line.indexOf(open, at);
        const stop = end === -1 ? line.length : end + 3;
        out.push(token(at, "tok-str", line.slice(at, stop)));
        if (end !== -1) open = null;
        at = stop;
        continue;
      }
      for (const [rule, className] of TOML_RULES) {
        rule.lastIndex = at;
        const match = rule.exec(line);
        if (!match) continue;
        if (className === "open") {
          open = match[0];
          out.push(token(at, "tok-str", open));
        } else {
          // Inside an inline table, a word before `=` is a key.
          const key = className === "word" && /^\s*=/.test(line.slice(at + match[0].length));
          out.push(token(at, key ? "tok-var" : className === "word" ? "" : className, match[0]));
        }
        at += match[0].length;
        break;
      }
    }
    return out;
  });
}
