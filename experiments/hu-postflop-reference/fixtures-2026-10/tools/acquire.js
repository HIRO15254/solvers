// GTO Wizard acquisition for the SOL-8 fixtures (see ../README.md).
//
// Runs in the omp eval JavaScript kernel against a logged-in GTO Wizard tab named "sol8".
// GTO Wizard allows 1,275 spot-solution requests per account per 24 h, shared with the
// user's own study; this module stops at DAILY_CAP calls per rolling 24 h and on the daily
// limit response. Every fetched node is cached under runs/sol8/cache so an interrupted
// crawl resumes without refetching.

const API = "https://api.gtowizard.com/v4/solutions/spot-solution/";
const CACHE = "runs/sol8/cache";
const BUDGET_FILE = "runs/sol8/budget.json";
export const DAILY_CAP = 900;
const PACE_MS = 800;
const TAB = "sol8";

const sleep = ms => new Promise(r => setTimeout(r, ms));
const tab = () => globalThis.browser.tab(TAB);

// Capture the app's bearer token from its own requests; install the fetch helper.
export async function installHook() {
  await tab().evaluate(`(() => {
    if (!window.__hooked) {
      const set = XMLHttpRequest.prototype.setRequestHeader;
      XMLHttpRequest.prototype.setRequestHeader = function (k, v) {
        if (/^authorization$/i.test(k) && /^Bearer /.test(v)) window.__auth = v;
        return set.call(this, k, v);
      };
      const f = window.fetch;
      window.fetch = function (input, init) {
        try {
          const h = init && init.headers;
          const v = h && (h instanceof Headers ? h.get('Authorization') : (h.Authorization || h.authorization));
          if (v && /^Bearer /.test(v)) window.__auth = v;
        } catch (e) {}
        return f.apply(this, arguments);
      };
      window.__hooked = true;
    }
    window.__sol8 = async (p) => {
      const q = new URLSearchParams({ gametype: '', depth: '', stacks: '', preflop_actions: '', flop_actions: '',
        turn_actions: '', river_actions: '', board: '', ...p });
      const res = await fetch('${API}?' + q, { headers: { Authorization: window.__auth } });
      return { status: res.status, body: res.status === 200 ? await res.text() : (await res.text()).slice(0, 300) };
    };
    return true;
  })()`);
}

// The token is short-lived: toggle the page's flop line so the app refetches with a fresh token.
export async function refreshAuth() {
  const t = tab();
  await t.evaluate(`window.__auth = null; true`);
  for (let k = 0; k < 3; k++) {
    const u = new URL(await t.url());
    const cur = u.searchParams.get("flop_actions") || "";
    u.searchParams.set("flop_actions", cur === "X" ? "" : "X");
    await t.pushState(u.toString());
    await t.evaluate(`window.dispatchEvent(new PopStateEvent('popstate')); true`);
    for (let i = 0; i < 20; i++) {
      if (await t.evaluate(`!!window.__auth`)) return;
      await sleep(500);
    }
  }
  throw new Error("auth refresh failed (logged out?)");
}

// Responses may contain bare NaN / Infinity.
const parse = body => JSON.parse(body.replace(/\bNaN\b/g, "null").replace(/-?\bInfinity\b/g, "null"));

export async function budget() {
  const f = Bun.file(BUDGET_FILE);
  const b = (await f.exists()) ? JSON.parse(await f.text()) : { calls: [] };
  const cutoff = Date.now() - 86400e3;
  b.calls = b.calls.filter(ts => ts > cutoff);
  return b;
}

async function fetchNode(params) {
  let n204 = 0, lastStatus = "";
  for (let attempt = 0; attempt < 6; attempt++) {
    const b = await budget();
    if (b.calls.length >= DAILY_CAP) throw new Error(`daily budget ${DAILY_CAP} reached`);
    b.calls.push(Date.now());
    await Bun.write(BUDGET_FILE, JSON.stringify(b));
    const r = await tab().evaluate(`window.__sol8(${JSON.stringify(params)})`);
    if (r.status === 200) return parse(r.body);
    if (r.status === 401) { await refreshAuth(); continue; }
    if (r.status === 204) { if (++n204 >= 2) return null; await sleep(1500); continue; } // unpublished node
    if (r.status === 429) {
      // Daily limit: stop. Otherwise a short-window limit: back off up to several minutes.
      if (/Request limit exceeded/.test(r.body)) throw new Error("GTO Wizard daily limit: " + r.body);
      lastStatus = `429 ${r.body}`;
      await sleep(30000 * (attempt + 1));
      continue;
    }
    throw new Error(`HTTP ${r.status}: ${r.body}`);
  }
  throw new Error(`retries exhausted (last ${lastStatus}) ` + JSON.stringify(params));
}

function players(j) {
  return j.players_info.map(p => ({
    pos: p.player.position,
    stack: p.player.current_stack,
    cot: p.player.chips_on_table,
    rel: p.player.relative_postflop_position,
    total_ev: p.total_ev,
    total_combos: p.total_combos,
  }));
}

function menu(j) {
  return j.action_solutions.map(a => ({
    code: a.action.code,
    betsize: a.action.betsize,
    next_street: a.action.next_street,
    hand_end: a.action.is_hand_end,
    freq: a.total_frequency,
    ev: a.total_ev,
  }));
}

// Summary: menu and aggregate frequencies. Full: plus per-combo ranges, EVs and strategies (GTO Wizard 1326 order).
function shape(j, full) {
  const node = { actor: j.game.active_position, pot: j.game.pot, street: j.game.current_street,
    players: players(j), menu: menu(j), warning: j.warning ?? null };
  if (full) {
    node.ranges = Object.fromEntries(j.players_info.map(p => [p.player.position, { range: p.range, hand_evs: p.hand_evs }]));
    node.actions = j.action_solutions.map(a => ({ code: a.action.code, strategy: a.strategy, evs: a.evs }));
  }
  return node;
}

async function node(id, params, full) {
  const key = [params.flop_actions ?? "", params.turn_actions ?? "", params.river_actions ?? "", params.board].join("|");
  const path = `${CACHE}/${id}.json`;
  const f = Bun.file(path);
  const cache = (await f.exists()) ? JSON.parse(await f.text()) : {};
  if (key in cache && (cache[key] === null || !full || cache[key].ranges)) return cache[key];
  const j = await fetchNode(params);
  await sleep(PACE_MS);
  cache[key] = j ? shape(j, full) : null;
  await Bun.write(path, JSON.stringify(cache));
  return cache[key];
}

// Breadth-first crawl of one street up to maxDepth decisions below its root.
async function crawl(id, base, street, board, prefix, full, maxDepth) {
  const key = { flop: "flop_actions", turn: "turn_actions", river: "river_actions" }[street];
  const out = { nodes: [], exits: [], missing: [], frontier: [] };
  const queue = [["", 0, 1]];
  while (queue.length) {
    const [seq, depth, weight] = queue.shift();
    const n = await node(id, { ...base, ...prefix, board, [key]: seq }, full);
    if (!n) { out.missing.push(seq); continue; }
    out.nodes.push({ seq, weight, ...n });
    for (const m of n.menu) {
      const next = seq ? `${seq}-${m.code}` : m.code;
      const w = weight * m.freq;
      if (m.hand_end) continue;
      if (m.next_street) out.exits.push({ seq: next, weight: w });
      else if (depth + 1 < maxDepth) queue.push([next, depth + 1, w]);
      else out.frontier.push(next);
    }
  }
  return out;
}

// Flop only (cached, so acquireCase later reuses it without new requests).
export async function acquireFlop(dir, spec) {
  const base = { gametype: spec.gametype, depth: String(spec.depth), preflop_actions: spec.gtow_preflop };
  const flop = await crawl(spec.id, base, "flop", spec.board, {}, true, 99);
  await Bun.write(`${dir}/gtow-flop.json`, JSON.stringify(flop));
  return { flop: flop.nodes.length, missing: flop.missing, exits: flop.exits.length };
}

// One case: every flop node with full data; turn menus two decisions deep after every flop exit
// on spec.turn; river menus two decisions deep after the RIVER_LINES most frequent turn exits on spec.river.
export const RIVER_LINES = 12;

export async function acquireCase(dir, spec) {
  const base = { gametype: spec.gametype, depth: String(spec.depth), preflop_actions: spec.gtow_preflop };
  const flop = await crawl(spec.id, base, "flop", spec.board, {}, true, 99);
  await Bun.write(`${dir}/gtow-flop.json`, JSON.stringify(flop));
  const turn = [], river = [], turnExits = [];
  const turnBoard = spec.board + spec.turn, riverBoard = turnBoard + spec.river;
  for (const fx of flop.exits) {
    const t = await crawl(spec.id, base, "turn", turnBoard, { flop_actions: fx.seq }, false, 2);
    for (const n of t.nodes) turn.push({ fa: fx.seq, ...n, weight: fx.weight * n.weight });
    for (const tx of t.exits) turnExits.push({ fa: fx.seq, ta: tx.seq, weight: fx.weight * tx.weight });
  }
  turnExits.sort((a, b) => b.weight - a.weight);
  for (const tx of turnExits.slice(0, RIVER_LINES)) {
    const r = await crawl(spec.id, base, "river", riverBoard, { flop_actions: tx.fa, turn_actions: tx.ta }, false, 2);
    for (const n of r.nodes) river.push({ fa: tx.fa, ta: tx.ta, ...n, weight: tx.weight * n.weight });
  }
  await Bun.write(`${dir}/gtow-menus.json`, JSON.stringify({ turn_card: spec.turn, river_card: spec.river, turn, river }));
  return { flop: flop.nodes.length, flop_missing: flop.missing, turn: turn.length, river: river.length };
}
