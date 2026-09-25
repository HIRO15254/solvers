"""Integrate explicit menu transcriptions and check River menu graph closure.

Default is read-only. --integrate updates observed.json from its first two
previously captured menus and all menu-capture-NN.{txt,json} records. It never
creates an unobserved menu; frontier entries remain missing observations.
"""
from decimal import Decimal
import argparse
import hashlib
import json
from pathlib import Path
import re
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

HERE = Path(__file__).resolve().parent
STACK = Decimal("97.5")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def decimal_text(value):
    text = format(Decimal(value), "f")
    return text.rstrip("0").rstrip(".") if "." in text else text


def raise_action(value):
    return "allin 97.5" if Decimal(value) == STACK else "raise to " + decimal_text(value)


def history_state(history):
    actor, amounts, names = 0, [Decimal(0), Decimal(0)], []
    last_increment = Decimal(1)
    for token in history.split("-") if history else []:
        if token == "X":
            require(amounts[actor] == amounts[1-actor], "check facing bet")
            names.append("check")
        else:
            require(token == "RAI" or re.fullmatch(r"R[0-9]+(?:\.[0-9]+)?", token), "invalid history token")
            target = STACK if token == "RAI" else Decimal(token[1:])
            require(target != STACK or token == "RAI", "numeric all-in history alias; captured history contract requires RAI")
            require(token == "RAI" or token == "R" + decimal_text(target), "noncanonical numeric history alias")
            require(amounts[1-actor] < target <= STACK, "non-increasing or oversized raise-to")
            increment = target - amounts[1-actor]
            require(target == STACK or increment >= last_increment, "history below minimum full raise")
            if increment >= last_increment:
                last_increment = increment
            names.append("allin 97.5" if token == "RAI" else
                         ("bet " if amounts[actor] == amounts[1-actor] else "raise to ") + str(target))
            amounts[actor] = target
        actor = 1-actor
    return "/".join(names), actor, amounts, last_increment


def parse_record(line):
    fields = line.split("|")
    require(len(fields) in (3, 4), "invalid transcription field count")
    history, extra, explicit_actor = fields[0], {}, None
    if len(fields) == 3:
        remaining, raises = fields[1:]
        actions = ["fold", "call"] + ([] if raises == "-" else
                   ["allin 97.5" if r == "RAI" else raise_action(r) for r in raises.split(",")])
    else:
        explicit_actor, remaining, source_actions = fields[1:]
        actions = []
        for token in source_actions.split(","):
            if token in ("F", "C"):
                actions.append({"F": "fold", "C": "call"}[token])
                continue
            match = re.fullmatch(r"(RAI|Allin|Raise|R)([0-9]+(?:\.[0-9]+)?)(?:\(([0-9]+(?:\.[0-9]+)?)%\))?", token)
            require(match, "invalid action token")
            kind, value, percent = match.groups()
            require(kind not in ("RAI", "Allin") or Decimal(value) == STACK, "all-in amount changed")
            action = "allin 97.5" if kind in ("RAI", "Allin") else raise_action(value)
            actions.append(action)
            if percent is not None:
                extra[action] = float(percent)
    require(actions[:2] == ["fold", "call"] and len(actions) == len(set(actions)), "response order/physical duplicates")
    text, actor, amounts, last_increment = history_state(history)
    require(explicit_actor is None or explicit_actor == ("BB", "BTN")[actor], "actor differs from captured history")
    require(Decimal(remaining) == STACK-amounts[actor], "displayed remaining stack differs from history")
    for action in actions[2:]:
        target = STACK if action == "allin 97.5" else Decimal(action.removeprefix("raise to "))
        require(amounts[1-actor] < target <= STACK, "invalid response raise-to")
        require(target == STACK or target - amounts[1-actor] >= last_increment,
                "response below minimum full raise")
    return history, text, actor, float(remaining), actions, extra


def check_percentages(row, incoming):
    known = dict(row.get("action_ui_percent_labels", {}))
    if "allin_97_5_percent" in row.get("extra_ui_labels", {}):
        value = row["extra_ui_labels"]["allin_97_5_percent"]
        require("allin 97.5" not in known or known["allin 97.5"] == value, "retained percentage conflict")
        known["allin 97.5"] = value
    for observation in row.get("additional_menu_observations", []):
        for action, percent in observation["action_ui_percent_labels"].items():
            require(action not in known or known[action] == percent, "retained percentage conflict")
            known[action] = percent
    for action, percent in incoming.items():
        require(action in row["actions"] and (action not in known or known[action] == percent),
                "reobserved percentage conflicts with retained label")


def assemble(observed):
    require(observed["case_id"] == "HU-R0-002" and observed["board"] == ["Ks", "7h", "2d", "3c", "8d"], "case mismatch")
    require(observed["condition_match"] == "unverified" and observed["quality_status"] == "not_evaluated"
            and observed["acceptance"] is None, "menu acquisition is not quality acceptance")
    menus = {}
    require(len(observed["observed_menus"]) >= 2, "both captured original root menus are required")
    for row, history in zip(observed["observed_menus"][:2], ("", "X")):
        require(row["source_history"] == history and row["actor"] == ("BB" if history == "" else "BTN")
                and row["actions"] == ["check", "bet 2", "bet 4", "bet 8.5", "allin 97.5"], "original root menu changed")
        menus[history] = row
    batches = []
    for metadata_path in sorted(HERE.glob("menu-capture-[0-9][0-9].json")):
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        data_name = metadata_path.with_suffix(".txt").name
        require(metadata["data_file"] == data_name and metadata["case_id"] == "HU-R0-002", "batch identity mismatch")
        data = (HERE/data_name).read_bytes()
        require(len(data) == metadata["bytes"] and hashlib.sha256(data).hexdigest() == metadata["sha256"], "batch bytes changed")
        lines = data.decode("ascii").splitlines()
        require(len(lines) == metadata["observed_record_count"], "batch record count mismatch")
        batches.append(metadata_path.name)
        for number, line in enumerate(lines, 1):
            history, text, actor, remaining, actions, percentages = parse_record(line)
            require(history not in menus, "duplicate history: " + history)
            url = urlsplit(observed["url"])
            query = dict(parse_qsl(url.query))
            query.update(river_actions=history, history_spot=str(10+len(history.split("-"))))
            row = {"history": text, "source_history": history, "actor": ("BB", "BTN")[actor],
                   "street": "river", "remaining_stack_bb_displayed": remaining, "actions": actions,
                   "source_transcription": data_name, "source_line": number,
                   "source_url_from_observed_history": urlunsplit((url.scheme, url.netloc, url.path, urlencode(query), "")),
                   "source_url_provenance": "Constructed from captured base URL and root-transcribed observed history; not an independently saved full DOM URL for every record.",
                   "observation_surface": metadata["source"]}
            if history in metadata.get("record_warnings", {}):
                row["reference_warning"] = metadata["record_warnings"][history]
            if history in metadata.get("record_extra_ui_labels", {}):
                row["extra_ui_labels"] = metadata["record_extra_ui_labels"][history]
            if percentages:
                row["action_ui_percent_labels"] = percentages
            menus[history] = row
        for line in metadata.get("reobserved_menus", []):
            history, _, actor, remaining, actions, percentages = parse_record(line)
            require(history in menus and menus[history]["actor"] == ("BB", "BTN")[actor]
                    and menus[history]["remaining_stack_bb_displayed"] == remaining
                    and menus[history]["actions"] == actions, "reobservation conflicts with retained menu")
            check_percentages(menus[history], percentages)
            menus[history].setdefault("additional_menu_observations", []).append({
                "source_transcription": metadata_path.name, "literal": line,
                "action_ui_percent_labels": percentages})
        for item in metadata.get("supplemental_action_labels", []):
            history = item["history"]
            require(history in menus and menus[history]["actor"] == item["actor"]
                    and menus[history]["remaining_stack_bb_displayed"] == item["remaining_stack_bb"],
                    "supplemental observation actor/remaining differs")
            check_percentages(menus[history], item["action_ui_percent_labels"])
            menus[history].setdefault("additional_menu_observations", []).append({
                "source_transcription": metadata_path.name, "scope": "Only the supplied action labels, not a newly complete menu capture",
                "action_ui_percent_labels": item["action_ui_percent_labels"]})
    frontier, incoming, terminal_edges = [], [], 0
    for history, row in menus.items():
        for action in row["actions"]:
            if action in ("fold", "call") or action == "check" and history == "X":
                terminal_edges += 1
                continue
            token = "X" if action == "check" else "RAI" if action == "allin 97.5" else "R"+action.split()[-1]
            child = history+"-"+token if history else token
            if child in menus:
                incoming.append(child)
            else:
                frontier.append(child)
    require(len(frontier) == len(set(frontier)), "duplicate frontier")
    require(sorted(incoming) == sorted(set(menus)-{""}), "orphan/duplicate observed decision")
    return list(menus.values()), frontier, batches, terminal_edges


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--integrate", action="store_true")
    args = parser.parse_args()
    path = HERE/"observed.json"
    observed = json.loads(path.read_text(encoding="utf-8"))
    menus, frontier, batches, terminal_edges = assemble(observed)
    summary = {"observed_menu_count": len(menus), "observed_terminal_edge_count": terminal_edges,
               "observed_public_node_count_if_terminal_edges_are_leaves": len(menus)+terminal_edges,
               "menu_graph_closed": not frontier, "unobserved_frontier_count": len(frontier),
               "scope": "Observed River decision menus and their fold/call/check-check endpoints only; no utility, complete policy, rake or reference-quality certification."}
    if args.integrate:
        observed["observed_menus"] = menus
        observed["unobserved_menu_frontier"] = frontier
        observed["menu_capture_batches"] = batches
        observed["menu_integrity_summary"] = summary
        observed["tree_observation"] = (
            f"{len(menus)} observed decision menus are retained ({len(menus)-2} explicit retranscribed tool-output records plus root and after-check). "
            + (f"{len(frontier)} directly reached decision histories remain unobserved; additional descendants may be missing. " if frontier else
               "Every nonterminal child in these observed menus has a retained menu; no unobserved frontier remains. ") +
            "Lost browser-memory observations are not adopted. Retracted observations require explicit recapture. "
            "Menu closure alone does not certify complete branch policies, game conditions or reference quality.")
        path.write_text(json.dumps(observed, ensure_ascii=False, indent=2)+"\n", encoding="utf-8", newline="\n")
    else:
        require(observed["observed_menus"] == menus and observed["unobserved_menu_frontier"] == frontier
                and observed["menu_capture_batches"] == batches and observed["menu_integrity_summary"] == summary,
                "observed menu projection/frontier/summary differs from raw transcriptions")
    print(json.dumps({"case_id": "HU-R0-002", "observed_menus": len(menus), "observed_terminal_edges": terminal_edges,
                      "menu_graph_closed": not frontier, "unobserved_menu_frontier": frontier,
                      "quality_status": "not_evaluated", "acceptance": None}, indent=2))


if __name__ == "__main__":
    main()
