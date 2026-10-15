/**
 * The default action colors, sampled from GTO Wizard's strategy views: fold
 * blue, check and call green, and bets and raises in four reds that darken
 * as the size grows.
 */
const FOLD = "#3d7cb8";
const PASSIVE = "#5ab966";
const AGGRESSIVE = ["#f03c3c", "#ca3232", "#a32929", "#7d1f1f"];

function mix(a: string, b: string, t: number): string {
  const channel = (hex: string, i: number) => parseInt(hex.slice(1 + 2 * i, 3 + 2 * i), 16);
  return `#${[0, 1, 2]
    .map((i) =>
      Math.round(channel(a, i) + (channel(b, i) - channel(a, i)) * t)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

/**
 * Colors for one node's action labels. Bets and raises take the reds by
 * their order in the menu, which runs from small to large: up to four take
 * one red each, more spread from the lightest to the darkest.
 */
export function actionColors(labels: string[]): string[] {
  const count = labels.filter((label) => label !== "fold" && label !== "check" && label !== "call").length;
  let rank = 0;
  return labels.map((label) => {
    if (label === "fold") return FOLD;
    if (label === "check" || label === "call") return PASSIVE;
    const i = rank++;
    if (count <= AGGRESSIVE.length) return AGGRESSIVE[i]!;
    const at = (i * (AGGRESSIVE.length - 1)) / (count - 1);
    const low = Math.floor(at);
    return low >= AGGRESSIVE.length - 1
      ? AGGRESSIVE[AGGRESSIVE.length - 1]!
      : mix(AGGRESSIVE[low]!, AGGRESSIVE[low + 1]!, at - low);
  });
}
