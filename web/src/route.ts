// Hash routes (`#/runs/<id>`): the daemon serves one static index for the
// GUI, so the location after `#` carries the page.

export type Route =
  { page: "setup" } | { page: "runs"; runId?: string } | { page: "results"; runId?: string } | { page: "guide" };

export function parseRoute(hash: string): Route {
  const [page, id] = hash.replace(/^#\/?/, "").split("/");
  const runId = id ? decodeURIComponent(id) : undefined;
  if (page === "runs") return { page, runId };
  if (page === "results") return { page, runId };
  if (page === "guide") return { page };
  return { page: "setup" };
}

export function navigate(route: Route): void {
  const id = "runId" in route && route.runId ? `/${encodeURIComponent(route.runId)}` : "";
  window.location.hash = `#/${route.page}${id}`;
}
