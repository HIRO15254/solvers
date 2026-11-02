import { createElement, type ReactNode } from "react";
import type * as ReactModule from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { LineCursor, LineOption } from "../api/types";
import { TurnPicker } from "./SetupStart";

const fixture = vi.hoisted(() => ({ stateIndex: 0, sizing: null as unknown }));
vi.mock("react", async (importOriginal) => {
  const react = await importOriginal<typeof ReactModule>();
  return {
    ...react,
    useState: (initial: unknown) => {
      const state = react.useState(initial);
      return fixture.stateIndex++ === 0 ? [fixture.sizing, state[1]] : state;
    },
  };
});
vi.mock("../cryst", () => {
  const container = ({ children }: { children?: ReactNode }) => createElement("div", null, children);
  return {
    Button: container, Card: container, Popover: container, PopoverContent: container, PopoverTrigger: container,
    Input: ({ placeholder, ...props }: { placeholder?: string; "aria-label"?: string }) =>
      createElement("input", { placeholder, "aria-label": props["aria-label"], readOnly: true }),
  };
});

describe("wager input bounds", () => {
  it.each(["bet", "raise"] as const)("shows %s input bounds in BB at a non-terminating chip scale", (kind) => {
    const option: LineOption = { kind, token: "r", min: 2 / 3, max: 5 / 3 };
    const cursor: LineCursor = {
      bbChips: 3, moves: [], line: "", actions: [], street: "preflop", pot: 1,
      complete: false, boardCards: null, players: 2, terminal: false, button: 0, seats: [],
      next: { seat: 0, position: "BTN", street: "preflop", toCall: 0, stack: 2, wager: 0, options: [option] },
    };
    fixture.stateIndex = 0;
    fixture.sizing = option;
    const html = renderToStaticMarkup(
      <TurnPicker turn={{ position: "BTN", cursor, passes: [] }} open busy={false} onOpenChange={() => {}} onPick={() => {}} />,
    );
    expect(html).toContain('placeholder="0.666667–1.666667"');
    expect(html).toContain(`aria-label="${kind} amount (BB, street total)"`);
  });
});
