export type AppRoute =
  | { screen: "setup" }
  | { screen: "solving"; jobId: string }
  | { screen: "results"; jobId: string }

const JOB_ID = "[A-Za-z0-9][A-Za-z0-9._:-]{0,127}"
const solvePattern = new RegExp(`^#/solve/(${JOB_ID})$`)
const resultsPattern = new RegExp(`^#/results/(${JOB_ID})$`)

export function routeFromHash(hash: string): AppRoute {
  const solve = solvePattern.exec(hash)
  if (solve) {
    return { screen: "solving", jobId: solve[1] }
  }
  const results = resultsPattern.exec(hash)
  if (results) {
    return { screen: "results", jobId: results[1] }
  }
  return { screen: "setup" }
}

export function hashForRoute(route: AppRoute) {
  if (route.screen === "setup") {
    return "#/setup"
  }
  return `#/${route.screen === "solving" ? "solve" : "results"}/${route.jobId}`
}

export function navigateTo(route: AppRoute) {
  window.location.hash = hashForRoute(route)
}
