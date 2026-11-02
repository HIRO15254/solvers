import { createElement, isValidElement, type ReactNode } from "react";
import type * as ReactModule from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PreviewTreeRow } from "../api/types";
import { TreeCard } from "./TreePreview";

// A small hook host lets these interaction tests run in Vitest's Node environment.
const host = vi.hoisted(() => ({
  slots: [] as unknown[], index: 0, effects: [] as (() => void)[],
  tree: vi.fn(),
}));
vi.mock("react", async (original) => {
  const react = await original<typeof ReactModule>();
  return {
    ...react,
    useState: (initial: unknown) => {
      const index = host.index++;
      if (!(index in host.slots)) host.slots[index] = initial;
      return [host.slots[index], (value: unknown) => {
        host.slots[index] = typeof value === "function" ? (value as (previous: unknown) => unknown)(host.slots[index]) : value;
      }];
    },
    useRef: (initial: unknown) => {
      const index = host.index++;
      if (!(index in host.slots)) host.slots[index] = { current: initial };
      return host.slots[index];
    },
    useMemo: (make: () => unknown) => make(),
    useEffect: (effect: () => void | (() => void), deps: unknown[]) => {
      const index = host.index++;
      const previous = host.slots[index] as { deps: unknown[]; cleanup?: () => void } | undefined;
      if (!previous || deps.some((value, i) => !Object.is(value, previous.deps[i]))) {
        host.effects.push(() => {
          previous?.cleanup?.();
          const cleanup = effect();
          host.slots[index] = { deps, cleanup: cleanup || undefined };
        });
      }
    },
  };
});
vi.mock("../cryst", () => {
  const container = ({ children }: { children?: ReactNode }) => createElement("div", null, children);
  return {
    Alert: container, Badge: container, Button: container, Card: container, CardAction: container,
    CardContent: container, CardHeader: container, CardTitle: container, EmptyState: container,
    Icon: container, IconButton: container, Input: container, Label: container, Popover: container,
    PopoverContent: container, PopoverTrigger: container,
  };
});
vi.mock("../api/client", () => ({ api: { tree: host.tree } }));

const flop: PreviewTreeRow = {
  history: "", kind: "action", street: "flop", actor: "oop", pot: 6,
  actions: ["check", "bet 2"], children: ["x", "b"], child_kinds: ["chance", "chance"],
};
const chance = (history: string, card: string, child = `${history}[${card}]`): PreviewTreeRow => ({
  history, kind: "chance", street: "turn", actor: null, pot: 6,
  actions: [card], children: [child], child_kinds: ["action"],
});
const turn = (history: string): PreviewTreeRow => ({
  history, kind: "action", street: "turn", actor: "oop", pot: 6,
  actions: ["check"], children: [`${history}x`], child_kinds: ["terminal"],
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function render(toml = "config", bbChips = 1000) {
  host.index = 0;
  const result = TreeCard({ toml, ready: true, roles: null, start: null, bbChips });
  for (const effect of host.effects.splice(0)) effect();
  return result;
}
function elements(node: ReactNode): { type: unknown; props: Record<string, unknown> }[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement(node)) return [];
  const element = node as { type: unknown; props: Record<string, unknown> };
  return [element, ...elements(element.props.children as ReactNode)];
}
function clickAction(tree: ReactNode, index: number) {
  const rows = elements(tree).filter((item) => item.props.className === "tree-row");
  (rows[index]!.props.onClick as () => void)();
}
function pick(tree: ReactNode, child: string) {
  const grid = elements(tree).find((item) => typeof item.props.onPick === "function");
  (grid!.props.onPick as (value: string) => void)(child);
}
async function initial() {
  host.tree.mockResolvedValueOnce([flop]);
  render();
  await vi.advanceTimersByTimeAsync(300);
  return render();
}
beforeEach(() => { host.slots = []; host.index = 0; host.effects = []; host.tree.mockReset(); vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });

describe("tree preview deals", () => {
  it("keeps the latest requested branch when responses arrive out of order", async () => {
    const tree = await initial();
    const first = deferred<PreviewTreeRow[]>();
    const second = deferred<PreviewTreeRow[]>();
    host.tree.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    clickAction(tree, 0);
    clickAction(tree, 1);
    second.resolve([chance("b", "Kh")]);
    await second.promise;
    first.resolve([chance("x", "As")]);
    await first.promise;
    const grid = elements(render()).find((item) => typeof item.props.onPick === "function");
    expect((grid!.props.row as PreviewTreeRow).actions).toEqual(["Kh"]);
  });

  it("shows a deal failure beside its branch without hiding the tree", async () => {
    const tree = await initial();
    const request = deferred<PreviewTreeRow[]>();
    host.tree.mockReturnValueOnce(request.promise);
    clickAction(tree, 0);
    request.reject(new Error("deal failed"));
    await request.promise.catch(() => undefined);
    const result = elements(render());
    expect(result.some((item) => item.props.role === "tree")).toBe(true);
    expect(result.some((item) => item.props.title === "Cannot deal card" && item.props.description === "deal failed")).toBe(true);
  });

  it("invalidates walked streets and reloads the root after the chip scale changes", async () => {
    const tree = await initial();
    host.tree.mockResolvedValueOnce([chance("x", "As")]);
    clickAction(tree, 0);
    await Promise.resolve();
    pick(render(), "x[As]");
    host.tree.mockResolvedValueOnce([flop, turn("x[As]")]);
    render();
    await vi.advanceTimersByTimeAsync(300);
    expect(elements(render()).filter((item) => item.props.className === "history-card")).toHaveLength(2);
    render("config", 3);
    host.tree.mockResolvedValueOnce([flop]);
    const reset = render("config", 3);
    expect(elements(reset).filter((item) => item.props.className === "history-card")).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(300);
    expect(host.tree).toHaveBeenLastCalledWith("config", ["root"], true);
  });
});
