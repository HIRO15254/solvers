import type { ArtifactEntry, RunSummary } from "../api/types";
import { setAt, type Doc } from "./config";

/** A daemon base replaces the CLI-only path in requests, not in the saved document. */
export function freezeRequestDoc(doc: Doc, freezeBaseRunId: string): Doc {
  return freezeBaseRunId ? setAt(doc, ["profile", "freeze_upstream"], undefined) : doc;
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
