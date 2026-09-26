"""Check the retained 022 decision graph, not terminal settlement or policy.

The packed bytes are reported browser observations. Child closure is checked
from every observed action; no decision menu is inferred from seat symmetry.
"""

import argparse
from collections import Counter
from decimal import Decimal
import hashlib
import json
import re
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

from check_ranges import CASE, HERE, fnv1a32, read_json, require

COPY_CHARACTERS = 6963
COPY_FNV = "05b8ce21"
CAPTURE_BEFORE = "2026-09-26T21:31:41Z"
STACK = Decimal(86)
AMOUNT = r"(?:0|[1-9]\d*)(?:\.\d+)?"
TOKEN = rf"(?:X|F|C|RAI|R{AMOUNT})"


def compact_bytes(packed):
    return json.dumps(packed, ensure_ascii=True, separators=(",", ":")).encode("ascii")


def number(value):
    return format(value, "f")


def state(history):
    """Replay only live decision prefixes; raise values are street totals."""
    contributions = [Decimal(0), Decimal(0)]
    tokens = history.split("-") if history else []
    previous = None
    for depth, token in enumerate(tokens):
        require(re.fullmatch(TOKEN, token), "invalid history token")
        actor = depth % 2
        if token == "X":
            require(contributions[actor] == contributions[1 - actor], "check facing a bet")
            require(previous != "X", "terminal check/check cannot be a decision row")
        else:
            require(token not in ("F", "C"), "terminal fold/call cannot be a decision row")
            target = STACK if token == "RAI" else Decimal(token[1:])
            require(max(contributions) < target <= STACK, "non-increasing or oversized raise")
            require(token == "RAI" or target < STACK, "86 must use the observed RAI identifier")
            contributions[actor] = target
        previous = token
    actor = len(tokens) % 2
    require(contributions[actor] < STACK, "no acting chips remain")
    return tokens, actor, contributions


def action(entry, index, history, contributions, actor):
    require(isinstance(entry, list) and len(entry) == 2 and all(isinstance(x, str) for x in entry),
            "malformed menu entry")
    identifier, text = entry
    match = re.fullmatch(rf"hspotcrd_action_({TOKEN})_(\d+)", identifier)
    require(match is not None and match[2] == str(index), "action id/index mismatch")
    token = match[1]
    facing_bet = contributions[actor] < contributions[1 - actor]
    if token in ("X", "F", "C"):
        require(text == {"X": "Check", "F": "Fold", "C": "Call"}[token], "action text mismatch")
        require(facing_bet == (token != "X"), "check/fold/call inconsistent with outstanding bet")
    else:
        target = STACK if token == "RAI" else Decimal(token[1:])
        require(max(contributions) < target <= STACK, "invalid menu raise target")
        require(token == "RAI" or target < STACK, "full-stack action must be RAI")
        label = "Allin" if token == "RAI" else ("Raise" if facing_bet else "Bet")
        match_text = re.fullmatch(rf"{label} ({AMOUNT}) \((\d+)%\)", text)
        require(match_text is not None and Decimal(match_text[1]) == target, "raise text/amount mismatch")
        # Percentages are literal UI labels. Their rounding rule is unobserved.
    terminal = {"F": "fold", "C": "call"}.get(token)
    if token == "X" and history == "X":
        terminal = "check_check"
    return token, terminal


def validate_graph(packed, observed):
    require(set(packed) == {"base_urls", "menus", "rows"}, "packed fields changed")
    bases, menus, rows = (packed[key] for key in ("base_urls", "menus", "rows"))
    require(isinstance(bases, list) and len(bases) == 1, "one common base URL required")
    parsed = urlsplit(bases[0])
    require(parsed.scheme == "https" and parsed.netloc == "app.gtowizard.com"
            and parsed.path == "/solutions" and not parsed.fragment, "unexpected base URL")
    params = parse_qsl(parsed.query, keep_blank_values=True)
    require(len(dict(params)) == len(params), "duplicate base URL parameter")
    query = dict(params)
    expected = {"soltab": "range", "solution_type": "gwiz", "gmfs_solution_tab": "ai_sols",
                "gametype": "Cash6m50zGeneral", "depth": "100", "gmfft_sort_key": "0",
                "gmfft_sort_order": "desc", "stratab": "strategy_ev",
                "preflop_actions": observed["preflop_actions"], "board": "".join(observed["board"]),
                "flop_actions": "X-X", "turn_actions": "X-X"}
    require(query == expected, "base URL does not match captured game/root")
    require(observed["pot_bb"] == Decimal("30.5") and observed["stacks_behind_bb"] == [86, 86]
            and observed["root_actor"] == "BB" and observed["seats"] == {"oop": "BB", "ip": "BTN"},
            "unexpected root dimensions")
    require(isinstance(menus, list) and menus and all(isinstance(m, list) and m for m in menus),
            "empty or malformed menus")
    require(len({compact_bytes(m) for m in menus}) == len(menus), "duplicate deduplicated menu")
    require(isinstance(rows, list) and rows, "empty observations")
    nodes, used_menus = {}, set()
    for row in rows:
        require(isinstance(row, list) and len(row) == 5, "malformed observation row")
        card, title, spot, url_history, menu_index = row
        require(isinstance(url_history, str) or url_history is None, "invalid URL history")
        require(url_history != "", "root history parameter must be absent, not empty")
        history = url_history or ""
        require(history not in nodes, "duplicate decision history")
        require(type(menu_index) is int and 0 <= menu_index < len(menus), "invalid menu index")
        tokens, actor, paid = state(history)
        seat, depth = ("BB", "BTN")[actor], len(tokens)
        require(spot == str(12 + depth) and card == f"hs_{spot}_river_{seat}_active",
                "selected card/actor/depth/history_spot mismatch")
        require(title == f"{seat} {number(STACK - paid[actor])}", "visible actor stack mismatch")
        decoded = [list(action(entry, i, history, paid, actor)) for i, entry in enumerate(menus[menu_index])]
        action_ids = [token for token, _ in decoded]
        require(len(set(action_ids)) == len(action_ids), "duplicate action token")
        facing_bet = paid[actor] < paid[1 - actor]
        require(action_ids[:2] == ["F", "C"] if facing_bet else action_ids[:1] == ["X"],
                "required fold/call or check menu prefix missing")
        if max(paid) == STACK:
            require(action_ids == ["F", "C"], "allin response must contain only fold/call")
        root_labels = [entry[1].split(" (")[0].lower() for entry in menus[menu_index]]
        if not history:
            require(root_labels == observed["observed_menus"][0]["actions"], "root menu observation differs")
        url_params = params + [("history_spot", spot)]
        if url_history is not None:
            url_params.append(("river_actions", url_history))
        nodes[history] = {"history": history, "actor": seat, "depth": depth,
                          "menu_index": menu_index, "street_contributions_bb": list(map(number, paid)),
                          "remaining_actor_stack_bb": number(STACK - paid[actor]),
                          "url": urlunsplit(parsed._replace(query=urlencode(url_params))),
                          "actions": decoded}
        used_menus.add(menu_index)
    require("" in nodes, "root missing")
    require(used_menus == set(range(len(menus))), "unreferenced menu")
    child_edges, terminals, incoming = [], [], Counter()
    for history, node in nodes.items():
        for token, kind in node["actions"]:
            child = "-".join(filter(None, (history, token)))
            if kind:
                require(child not in nodes, "terminal edge has a decision observation")
                terminals.append({"history": child, "parent": history, "actor": node["actor"],
                                  "kind": kind, "terminal_ui_visited": False, "settlement_observed": False})
            else:
                require(child in nodes, "missing decision child: " + child)
                child_edges.append({"parent": history, "action": token, "child": child})
                incoming[child] += 1
    require(all(incoming[h] == 1 for h in nodes if h) and incoming[""] == 0,
            "orphan or multiply reached decision observation")
    require(len({x["history"] for x in terminals}) == len(terminals), "duplicate terminal history")
    counts = dict(sorted(Counter(t["kind"] for t in terminals).items()))
    return {"decision_observations": len(nodes), "unique_menus": len(menus),
            "action_edges": len(child_edges) + len(terminals), "decision_edges": len(child_edges),
            "derived_terminals": len(terminals), "terminal_kind_counts": counts,
            "maximum_decision_depth": max(n["depth"] for n in nodes.values()),
            "maximum_terminal_depth": max(t["history"].count("-") + 1 for t in terminals),
            "decision_nodes": list(nodes.values()), "decision_edges_table": child_edges,
            "derived_terminal_table": terminals}


def pin(path):
    raw = path.read_bytes()
    return {"file": path.name, "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def calculate(directory=HERE):
    recorded, observed = (read_json(directory / name) for name in ("menus.json", "observed.json"))
    require(recorded["case_id"] == observed["case_id"] == CASE, "case mismatch")
    require(recorded["capture_date_utc"] == "2026-09-26"
            and recorded["capture_completed_before_utc"] == CAPTURE_BEFORE, "capture bound mismatch")
    require(recorded["row_columns"] == ["selected_card_data_tst", "visible_title", "url_history_spot",
            "url_river_actions", "menu_index"] and recorded["menu_entry_columns"] == ["action_data_tst", "visible_text"],
            "packed column contract changed")
    copied = compact_bytes(recorded["packed"])
    require(recorded["transfer_check"]["characters"] == COPY_CHARACTERS
            and recorded["transfer_check"]["fnv1a32"] == COPY_FNV, "reported transfer pin changed")
    require(len(copied) == COPY_CHARACTERS and fnv1a32(copied) == COPY_FNV, "packed transfer mismatch")
    require(observed["condition_match"] == "unverified" and observed["acceptance"] is None
            and observed["quality_status"] == "not_evaluated", "menu closure must not certify quality")
    graph = validate_graph(recorded["packed"], observed)
    require(graph["decision_observations"] == 72 and graph["unique_menus"] == 18, "capture count mismatch")
    require(observed["url"] == graph["decision_nodes"][0]["url"] and not graph["decision_nodes"][0]["history"],
            "root captured URL mismatch")
    return {"schema": "r1.reference-menu-closure/v1", "case_id": CASE,
            "inputs": [pin(directory / name) for name in ("menus.json", "observed.json", "check_menus.py", "check_ranges.py")],
            "packed_ascii_characters": len(copied), "packed_fnv1a32": fnv1a32(copied),
            "packed_sha256": hashlib.sha256(copied).hexdigest(),
            "capture_completed_before_utc": CAPTURE_BEFORE, "decision_menu_closure": "verified",
            "checks": ["exact reported packed transfer", "distinct histories and deduplicated menus",
                       "URL game fields", "selected card, actor, depth and history_spot",
                       "street-total raise replay and visible remaining stack", "action identifier/index/text",
                       "all nonterminal children observed and no orphan rows", "unique inferred terminal histories"],
            "terminal_observation_scope": "Fold/Call/second-Check are derived terminal edges, not clicked outcomes; no settlement was observed",
            "percentage_label_scope": "literal UI labels only; exact rounding rule is unknown",
            "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None,
            **graph}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="create menu-check.json only if absent")
    args = parser.parse_args()
    result = calculate()
    path = HERE / "menu-check.json"
    if args.write:
        with path.open("x", encoding="utf-8", newline="\n") as output:
            output.write(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    else:
        require(read_json(path) == result, "retained menu-check.json differs from source bytes/graph")
    print(json.dumps({key: result[key] for key in ("case_id", "decision_menu_closure", "decision_observations",
        "unique_menus", "action_edges", "derived_terminals", "terminal_kind_counts", "quality_status")}))


if __name__ == "__main__":
    main()
