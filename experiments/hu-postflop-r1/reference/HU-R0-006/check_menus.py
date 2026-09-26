"""Check 006 selected decision menus without mistaking stale URL suffixes for visits.

No browser, solver, or external settlement is executed. Transfer pins identify
the collecting root's completed browser capture, not an external policy proof.
"""

import argparse
from collections import Counter
from decimal import Decimal
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

from check_ranges import HERE, fnv1a32, read_json, require

SCHEMA = "hu-river-ui-menus/v1"
CASE = "HU-R0-006"
STACK = Decimal(97)
MIN_BET_ASSUMPTION = Decimal(1)
ROOT_SPOT = 10
AMOUNT = r"(?:0|[1-9]\d*)(?:\.\d+)?"
TOKEN = rf"(?:X|F|C|RAI|R{AMOUNT})"
CAPTURE_PIN = (10717, "a2150af3")
CAPTURE_AFTER = "2026-09-26T21:55:42Z"
CAPTURE_BEFORE = "2026-09-26T22:14:54Z"


def compact_bytes(document):
    return json.dumps(document, ensure_ascii=True, separators=(",", ":")).encode("ascii")


def transfer_check(document):
    raw = compact_bytes(document)
    result = {"ascii_characters": len(raw), "fnv1a32": fnv1a32(raw),
              "sha256": hashlib.sha256(raw).hexdigest(),
              "browser_pin_comparison": "not_supplied"}
    if CAPTURE_PIN is not None:
        require((len(raw), fnv1a32(raw)) == CAPTURE_PIN, "browser transfer pin mismatch")
        result["browser_pin_comparison"] = "verified"
    return result


def tokens(text):
    require(isinstance(text, str), "history must be a string")
    result = text.split("-") if text else []
    require(all(re.fullmatch(TOKEN, token) for token in result), "invalid history token")
    return result


def validate_capture_window(window):
    require(isinstance(window, dict) and set(window) == {"after_utc", "completed_before_utc", "exact_per_node_times", "method"},
            "capture window fields differ")
    parsed = []
    for field in ("after_utc", "completed_before_utc"):
        value = window[field]
        require(isinstance(value, str) and re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", value),
                "capture bound must be a UTC timestamp")
        parsed.append(datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc))
    require(parsed[0] < parsed[1], "capture bounds are not ordered")
    require(window["after_utc"] == CAPTURE_AFTER and window["completed_before_utc"] == CAPTURE_BEFORE,
            "capture bounds differ from supplied clock observations")
    require(window["exact_per_node_times"] is None, "exact per-node times were not captured")
    require(isinstance(window["method"], str) and bool(window["method"].strip()), "capture method is absent")


def check_minimum_target(token, amount, paid, actor, last_increment):
    minimum = paid[1 - actor] + (last_increment or MIN_BET_ASSUMPTION)
    require(amount >= minimum or (token == "RAI" and amount == STACK),
            "non-allin target is below the minimum full bet/raise")


def live_state(chosen):
    paid = [Decimal(0), Decimal(0)]
    previous = None
    last_increment = Decimal(0)
    for index, token in enumerate(chosen):
        actor = index % 2
        require(token not in ("F", "C"), "terminal action cannot be in selected decision prefix")
        if token == "X":
            require(paid[actor] == paid[1 - actor] and previous != "X",
                    "selected prefix checks into a bet or past check/check")
        else:
            amount = STACK if token == "RAI" else Decimal(token[1:])
            require(max(paid) < amount <= STACK and (token == "RAI" or amount < STACK),
                    "selected prefix has invalid cumulative raise target")
            require(amount * 100 == (amount * 100).to_integral_value(), "target has fractional chips")
            check_minimum_target(token, amount, paid, actor, last_increment)
            last_increment = amount - paid[1 - actor]
            paid[actor] = amount
        previous = token
    actor = len(chosen) % 2
    require(paid[actor] < STACK, "selected actor has no chips remaining")
    return actor, paid, last_increment


def selected_state(path, spot, raw_history):
    require(type(spot) is int and spot >= ROOT_SPOT, "spot must be an integer at least 10")
    chosen, raw = tokens(path), tokens(raw_history)
    depth = spot - ROOT_SPOT
    require(len(chosen) == depth and len(raw) >= depth and chosen == raw[:depth],
            "selected path is not the exact history_spot prefix")
    actor, paid, _ = live_state(chosen)
    # The retained suffix is syntax checked, not replayed as visited nodes.
    return actor, paid, raw[depth:]


def decode_menu(entries, actor, paid, path):
    require(isinstance(entries, list) and entries, "empty/malformed menu")
    replay_actor, replay_paid, last_increment = live_state(tokens(path))
    require(actor == replay_actor and paid == replay_paid, "menu state contradicts selected history")
    facing = paid[actor] < paid[1 - actor]
    result = []
    for index, entry in enumerate(entries):
        require(isinstance(entry, list) and len(entry) == 2 and all(isinstance(x, str) for x in entry),
                "malformed menu entry")
        identifier, text = entry
        match = re.fullmatch(rf"hspotcrd_action_({TOKEN})_(\d+)", identifier)
        require(match is not None and match[2] == str(index), "action id/index mismatch")
        token = match[1]
        require(text == text.strip(), "visible action text must be trimmed")
        if token in ("F", "C", "X"):
            require(text == {"F": "Fold", "C": "Call", "X": "Check"}[token], "passive action text mismatch")
            require(facing == (token != "X"), "passive action contradicts outstanding bet")
        else:
            amount = STACK if token == "RAI" else Decimal(token[1:])
            require(max(paid) < amount <= STACK and (token == "RAI" or amount < STACK),
                    "menu has invalid cumulative target")
            require(amount * 100 == (amount * 100).to_integral_value(), "target has fractional chips")
            check_minimum_target(token, amount, paid, actor, last_increment)
            label = "Allin" if token == "RAI" else "Raise" if facing else "Bet"
            visible = re.fullmatch(rf"{label} ({AMOUNT}) \((\d+)%\)", text)
            require(visible is not None and Decimal(visible[1]) == amount, "action label/amount mismatch")
        terminal = {"F": "fold", "C": "call"}.get(token)
        if token == "X" and path == "X":
            terminal = "check_check"
        result.append([token, terminal])
    action_tokens = [token for token, _ in result]
    require(len(set(action_tokens)) == len(action_tokens), "duplicate menu action")
    require(action_tokens[:2] == ["F", "C"] if facing else action_tokens[:1] == ["X"],
            "required passive menu prefix missing")
    if max(paid) == STACK:
        require(action_tokens == ["F", "C"], "allin response must be fold/call only")
    return result


def node_url(base, spot, history):
    url = urlsplit(base)
    params = [(key, str(spot) if key == "history_spot" else value) for key, value in parse_qsl(url.query)]
    if history:
        params.append(("river_actions", history))
    return urlunsplit(url._replace(query=urlencode(params)))


def analyze(document, observed):
    require(document["schema"] == SCHEMA and document["case_id"] == observed["case_id"] == CASE, "schema/case mismatch")
    require(document["base_url"] == observed["url"], "base URL differs from frozen root observation")
    validate_capture_window(document["capture_window"])
    params = parse_qsl(urlsplit(document["base_url"]).query)
    require(len(dict(params)) == len(params) and dict(params).get("history_spot") == "10"
            and "river_actions" not in dict(params), "base URL has duplicate/nonroot history parameters")
    require(observed["seats"] == {"oop": "SB", "ip": "BB"} and observed["pot_bb"] == 6
            and observed["stacks_behind_bb"] == [97, 97], "root game mismatch")
    require(observed["condition_match"] == "unverified" and observed["quality_status"] == "not_evaluated"
            and observed["acceptance"] is None, "menu closure cannot certify external quality")
    menus, rows = document["menu_sets"], document["rows"]
    require(isinstance(menus, list) and menus and all(isinstance(menu, list) and menu for menu in menus), "invalid menu sets")
    require(len({compact_bytes(menu) for menu in menus}) == len(menus), "duplicate deduplicated menu")
    require(isinstance(rows, list) and rows, "no decision observations")
    nodes, used_menus = {}, set()
    for row in rows:
        require(isinstance(row, list) and len(row) == 5, "malformed observation row")
        path, spot, title, index, raw_history = row
        actor, paid, suffix = selected_state(path, spot, raw_history)
        require(path not in nodes, "duplicate selected decision path")
        require(type(index) is int and 0 <= index < len(menus), "invalid menu index")
        seat = ("SB", "BB")[actor]
        require(title == f"{seat} {format(STACK - paid[actor], 'f')}", "selected title actor/remaining stack mismatch")
        decoded = decode_menu(menus[index], actor, paid, path)
        _, _, last_increment = live_state(tokens(path))
        if not path:
            require([text.split(" (")[0].lower() for _, text in menus[index]] == observed["observed_menus"][0]["actions"],
                    "root menu differs from frozen root observation")
        nodes[path] = {"path": path, "history_spot": spot, "actor": seat, "menu_index": index,
                       "contributions_chips": [int(value * 100) for value in paid],
                       "pot_chips": 600 + int(sum(paid) * 100), "remaining_actor_stack_chips": int((STACK - paid[actor]) * 100),
                       "last_raise_increment_chips": int(last_increment * 100),
                       "minimum_full_target_chips": int((paid[1 - actor] + (last_increment or MIN_BET_ASSUMPTION)) * 100),
                       "raw_river_actions": raw_history, "unselected_url_suffix": suffix,
                       "observed_url_reconstructed": node_url(document["base_url"], spot, raw_history),
                       "canonical_selected_url": node_url(document["base_url"], spot, path), "actions": decoded}
        used_menus.add(index)
    require("" in nodes, "root decision missing")
    require(used_menus == set(range(len(menus))), "unreferenced menu set")
    incoming, terminals = Counter(), []
    edges = 0
    for path, node in nodes.items():
        for token, terminal in node["actions"]:
            edges += 1
            child = "-".join(filter(None, (path, token)))
            if terminal:
                require(child not in nodes, "derived terminal has a decision observation")
                terminals.append({"path": child, "kind": terminal, "actor": node["actor"],
                                  "terminal_ui_visited": False, "settlement_observed": False})
            else:
                require(child in nodes, "missing observed nonterminal child: " + child)
                incoming[child] += 1
    require(incoming[""] == 0 and all(incoming[path] == 1 for path in nodes if path), "orphan or multiply reached selected path")
    require(len({row["path"] for row in terminals}) == len(terminals), "duplicate terminal path")
    counts = dict(sorted(Counter(row["kind"] for row in terminals).items()))
    decisions = len(nodes)
    require(counts == {"call": decisions - 2, "check_check": 1, "fold": decisions - 2}
            and edges == 3 * decisions - 4, "river full-menu count identity differs")
    return {"decision_nodes": decisions, "unique_menus": len(menus), "action_edges": edges,
            "decision_edges": decisions - 1, "derived_terminal_nodes": len(terminals),
            "terminal_kind_counts": counts, "public_nodes": decisions + len(terminals),
            "maximum_decision_depth": max(node["history_spot"] - ROOT_SPOT for node in nodes.values()),
            "maximum_terminal_depth": max(len(tokens(row["path"])) for row in terminals),
            "maximum_aggressions": max(sum(token.startswith("R") for token in tokens(path)) for path in nodes),
            "short_allin_edges": sum(node["minimum_full_target_chips"] > int(STACK * 100)
                                     and any(token == "RAI" for token, _ in node["actions"]) for node in nodes.values()),
            "rows_with_unselected_suffix": sum(bool(node["unselected_url_suffix"]) for node in nodes.values()),
            "decision_nodes_table": list(nodes.values()), "derived_terminals": terminals}


def pin(path):
    raw = path.read_bytes()
    return {"file": path.name, "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def calculate(path=HERE / "menus.json"):
    document = read_json(path)
    raw = path.read_bytes()
    require(raw == compact_bytes(document) + b"\n", "capture file must be original compact ASCII JSON plus one LF")
    transfer = transfer_check(document)
    observed = read_json(HERE / "observed.json")
    frozen = read_json(HERE / "ranges.json")["observed"]
    require(pin(HERE / "observed.json") == frozen, "frozen root observation bytes changed")
    graph = analyze(document, observed)
    require(graph["decision_nodes"] == 120 and graph["unique_menus"] == 27, "completed capture counts differ")
    return {"schema": "r1.reference-menu-closure/v1", "case_id": CASE,
            "inputs": [pin(file) for file in (path, HERE / "observed.json", HERE / "ranges.json",
                       HERE / "check_menus.py", HERE / "check_ranges.py", HERE / "test_check_menus.py")],
            "capture_window": document["capture_window"], "transfer": transfer,
            "decision_menu_closure": "verified", "condition_match": "unverified", "quality_status": "not_evaluated", "acceptance": None,
            "native_validation": "not_executed", "solve": "not_executed",
            "minimum_bet_scope": "First bet >=1bb is an explicit NLH/runtime assumption, not a minimum observed in the UI. Full-raise minimum is prior opposing cumulative target plus the last observed raise increment; only all-in at97bb may be shorter.",
            "scope": "Only rows selected in the UI establish visits. Raw URL suffixes are retained but never synthesize nodes. Fold/Call/second Check are derived unvisited terminals. Percentage labels are literal; rounding/settlement/policy unknown.",
            "earlier_root_range_record": "unchanged; its graph_closure_evaluated=false describes that earlier range-only scope", **graph}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=HERE / "menus.json")
    parser.add_argument("--output", type=Path, help="create a new report without overwriting existing evidence")
    args = parser.parse_args()
    result = calculate(args.input)
    rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            output.write(rendered)
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
