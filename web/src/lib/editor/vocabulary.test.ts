import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { KEYWORDS, STREETS, VARIABLES } from "./vocabulary";

// The Rust dialect is the source of truth; the editor's copy must name the same words.
const read = (path: string) => readFileSync(new URL(`../../../../${path}`, import.meta.url), "utf8");

describe("vocabulary", () => {
  it("lists every condition variable of crates/spot/src/dialect.rs with its kind and board flag", () => {
    const rust = [...read("crates/spot/src/dialect.rs").matchAll(/\("(\w+)", (Bool|Number|Text), (true|false)\)/g)].map(
      ([, name, kind, board]) => `${name}:${kind!.toLowerCase()}:${board}`,
    );
    expect(rust.length).toBeGreaterThan(0);
    expect(VARIABLES.map((v) => `${v.name}:${v.kind}:${v.board}`)).toEqual(rust);
  });

  it("lists every reserved word of crates/nlh/src/script/cond.rs and every street", () => {
    const source = read("crates/nlh/src/script/cond.rs");
    const list = /const KEYWORDS: &\[&str\] = &\[([\s\S]*?)\];/.exec(source)?.[1] ?? "";
    const rust = [...list.matchAll(/"([^"]+)"/g)].map(([, word]) => word!);
    expect(rust.length).toBeGreaterThan(0);
    expect([...KEYWORDS].sort()).toEqual([...rust, ...STREETS].sort());
  });
});
