import { useEffect, useState, type ReactNode } from "react";
import type { RunState } from "../api/types";
import { Alert, Badge, Icon, Input, Label, Popover, PopoverContent, PopoverTrigger, type InputProps } from "../cryst";
import { SUIT_ICON } from "../lib/cards";

/**
 * A text input that keeps what is typed (a trailing space, a half-written
 * `0.`) while the document holds what the text reads as. `read` gives that
 * reading in the form `value` is written; a `value` that no longer matches
 * the typed text came from elsewhere (a template, a preset, the TOML step)
 * and replaces it. `onText` may return an error to show under the field;
 * `issue` is the validator's reason for the written value, shown when the
 * typed text has no error of its own.
 */
export function DraftInput({
  value,
  read,
  onText,
  issue,
  ...input
}: Omit<InputProps, "value" | "onChange" | "error"> & {
  value: string;
  read: (text: string) => string;
  onText: (text: string) => string | null | void;
  issue?: string;
}) {
  const [text, setText] = useState(value);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (read(text) !== value) {
      setText(value);
      setError(null);
    }
  }, [value]);
  return (
    <Input
      {...input}
      value={text}
      error={error ?? issue}
      onChange={(event) => {
        setText(event.target.value);
        setError(onText(event.target.value) ?? null);
      }}
    />
  );
}

/** One card in the four-color deck: the `suit-*` class colors its rank and suit. */
export function PlayingCard({ card, small = false }: { card: string; small?: boolean }) {
  const suit = card[1] ?? "";
  const icon = SUIT_ICON[suit];
  return (
    <span className={`playing-card suit-${suit} ${small ? "small" : ""}`} aria-label={card}>
      {card[0]}
      {icon ? <Icon name={icon} filled size={small ? 12 : 14} /> : suit}
    </span>
  );
}

/** Board cards; `ignored` follow dimmed: cards the line does not reach, left out of the spot. */
export function Board({
  cards,
  ignored = [],
  small = false,
}: {
  cards: string[];
  ignored?: string[];
  small?: boolean;
}) {
  return (
    <span className="row" style={{ gap: 4 }}>
      {cards.map((card) => (
        <PlayingCard key={card} card={card} small={small} />
      ))}
      {ignored.map((card) => (
        <IgnoredCard key={card} card={card} small={small} />
      ))}
    </span>
  );
}

/** A board card past the cards the line deals: shown, struck and dimmed, and not part of the spot. */
export function IgnoredCard({ card, small = false }: { card: string; small?: boolean }) {
  return (
    <span className="ignored-card" aria-disabled title={`${card}: the line does not deal this card, so it is ignored`}>
      <PlayingCard card={card} small={small} />
    </span>
  );
}

/** The ⓘ button beside a label; the explanation opens in a Popover. */
export function InfoHint({
  label,
  text,
  align = "start",
}: {
  label: string;
  text: ReactNode;
  align?: "start" | "end";
}) {
  return (
    <Popover align={align}>
      <PopoverTrigger>
        <button type="button" className="info-btn" aria-label={`About ${label}`}>
          <Icon name="info-circle" size={14} />
        </button>
      </PopoverTrigger>
      <PopoverContent>
        <div className="info-text">{text}</div>
      </PopoverContent>
    </Popover>
  );
}

/** A form field whose label carries an InfoHint; plain fields use the Cryst control's own `label`. */
export function Field({
  label,
  hint,
  htmlFor,
  className,
  children,
}: {
  label: string;
  hint: ReactNode;
  htmlFor?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={`field ${className ?? ""}`}>
      <div className="field-label">
        <Label htmlFor={htmlFor}>{label}</Label>
        <InfoHint label={label} text={hint} />
      </div>
      {children}
    </div>
  );
}

/** A muted uppercase heading over a group of key/value rows. */
export function Section({ title, hint, children }: { title: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <section className="section">
      <div className="field-label">
        <h2 className="overline">{title}</h2>
        {hint && <InfoHint label={title} text={hint} align="end" />}
      </div>
      {children}
    </section>
  );
}

export function KeyValue({ label, mono = true, children }: { label: string; mono?: boolean; children: ReactNode }) {
  return (
    <div className="kv">
      <span>{label}</span>
      <span className={mono ? "mono" : undefined}>{children}</span>
    </div>
  );
}

const STATE_BADGE: Record<
  RunState,
  { variant: "neutral" | "primary" | "success" | "warning" | "destructive"; icon: string }
> = {
  queued: { variant: "neutral", icon: "hourglass" },
  running: { variant: "primary", icon: "loader-2" },
  completed: { variant: "success", icon: "circle-check" },
  failed: { variant: "destructive", icon: "circle-x" },
  canceled: { variant: "neutral", icon: "ban" },
  interrupted: { variant: "warning", icon: "player-pause" },
};

export function StateBadge({ state }: { state: RunState }) {
  const { variant, icon } = STATE_BADGE[state];
  return (
    <Badge variant={variant}>
      <Icon name={icon} size={12} className={state === "running" ? "spin" : undefined} />
      {state}
    </Badge>
  );
}

/**
 * A Cryst Alert on its status tint. Alerts take the `card` fill, which vanishes
 * on the cards they sit on; the tint is the one Badge uses. Always titled: a
 * title aligns with the icon, a bare description sits below it.
 */
export function Notice({
  variant,
  title,
  description,
  actions,
  children,
  className,
}: {
  variant: "info" | "success" | "warning" | "destructive";
  title: ReactNode;
  description?: ReactNode;
  /** Buttons under the text. */
  actions?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <Alert
      variant={variant}
      title={title}
      description={description}
      actions={actions}
      className={`notice notice-${variant} ${className ?? ""}`}
    >
      {children}
    </Alert>
  );
}
