"use client";

import { type InputHTMLAttributes, useState } from "react";
import { parseSizingListDraft } from "./preflop-config";

interface Props
  extends Omit<
    InputHTMLAttributes<HTMLInputElement>,
    "value" | "defaultValue" | "onChange" | "onBlur"
  > {
  values: number[];
  onCommit: (values: number[]) => void;
}

interface DraftProps extends Omit<Props, "values"> {
  serialized: string;
}

function formatValues(values: number[]): string {
  return values.join(", ");
}

/**
 * Keeps incomplete decimal input (for example `0.` or a trailing comma)
 * intact while the user is typing. Numeric settings are updated only when
 * the field is committed with blur or Enter.
 */
function DraftInput({
  serialized,
  onCommit,
  onKeyDown,
  ...inputProps
}: DraftProps) {
  const [draft, setDraft] = useState(serialized);


  function commit() {
    const parsed = parseSizingListDraft(draft);
    onCommit(parsed);
    setDraft(formatValues(parsed));
  }

  return (
    <input
      {...inputProps}
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        onKeyDown?.(event);
        if (event.defaultPrevented) return;
        if (event.key === "Enter") {
          event.preventDefault();
          event.currentTarget.blur();
        } else if (event.key === "Escape") {
          event.preventDefault();
          setDraft(serialized);
        }
      }}
    />
  );
}

export default function CommaListInput({ values, ...inputProps }: Props) {
  const serialized = formatValues(values);
  return (
    <DraftInput
      key={serialized}
      serialized={serialized}
      {...inputProps}
    />
  );
}
