import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "./client";

afterEach(() => vi.unstubAllGlobals());

describe("Setup freeze base requests", () => {
  it("sends the selected base to validation and submission, but omits it when unset", async () => {
    vi.stubGlobal("localStorage", { getItem: () => null });
    const fetch = vi.fn().mockResolvedValue({ ok: true, json: async () => ({}) });
    vi.stubGlobal("fetch", fetch);
    await api.validate("config", true, "base");
    await api.createRun("config", "new", "base");
    await api.validate("config", true);
    await api.createRun("config", "new");
    expect(JSON.parse(fetch.mock.calls[0]![1].body)).toEqual({ configToml: "config", resources: true, freezeBaseRunId: "base" });
    expect(JSON.parse(fetch.mock.calls[1]![1].body)).toEqual({ configToml: "config", runId: "new", freezeBaseRunId: "base" });
    expect(JSON.parse(fetch.mock.calls[2]![1].body)).toEqual({ configToml: "config", resources: true });
    expect(JSON.parse(fetch.mock.calls[3]![1].body)).toEqual({ configToml: "config", runId: "new" });
  });
});
