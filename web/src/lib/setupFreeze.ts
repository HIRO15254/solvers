import type { ArtifactEntry, RunSummary } from "../api/types";
import { setAt, type Doc } from "./config";

/** A daemon base replaces the CLI-only path in requests, not in the saved document. */
export function freezeRequestDoc(doc: Doc, freezeBaseRunId: string): Doc {
  return freezeBaseRunId ? setAt(doc, ["profile", "freeze_upstream"], undefined) : doc;
}

/**
 * The daemon run a recorded `freeze_upstream` points at. The daemon writes
 * `<runs root>/<run id>/solution.sol`, so the directory names the run; a
 * path of another shape (a CLI file) names none.
 */
export function freezeBaseRun(freeze: unknown): string | undefined {
  const solution = typeof freeze === "object" && freeze !== null ? (freeze as Record<string, unknown>).solution : undefined;
  if (typeof solution !== "string") return undefined;
  const parts = solution.split(/[\\/]/);
  return parts.at(-1) === "solution.sol" ? parts.at(-2) || undefined : undefined;
}

/** Only stopped P1 runs with a saved solution can be used as freeze bases. */
export function isFreezeBase(run: RunSummary, artifacts: ArtifactEntry[]): boolean {
  return run.gameKind === "hu-postflop"
    && (run.state === "completed" || run.state === "canceled" || run.state === "interrupted")
    && artifacts.some((artifact) => artifact.name === "solution.sol");
}

export async function loadFreezeBases(
  listRuns: () => Promise<RunSummary[]>,
  listArtifacts: (id: string) => Promise<ArtifactEntry[]>,
): Promise<RunSummary[]> {
  const runs = (await listRuns()).filter((run) => run.gameKind === "hu-postflop"
    && (run.state === "completed" || run.state === "canceled" || run.state === "interrupted"));
  const candidates = await Promise.all(runs.map(async (run) => isFreezeBase(run, await listArtifacts(run.runId)) ? run : null));
  return candidates.filter((run): run is RunSummary => run !== null);
}
