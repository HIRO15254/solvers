"""Pure checks for finite, raked River diagnostics; no external acceptance gate.

The caller retains and hashes every input/output.  These functions do not run
processes, read files, or independently compute BLAKE3.  Reported BLAKE3 values
are native claims; the config hash can be bound to the independently read SOL
header by the caller.  SHA-256 artifact binding belongs to that caller.
"""

from collections import Counter
from copy import deepcopy
import math
import re
import tomllib


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value, name, minimum=0):
    require(type(value) is int and value >= minimum, f"invalid integer: {name}")
    return value


def finite(value, name):
    require(type(value) in (int, float) and math.isfinite(value), f"nonfinite: {name}")
    return value


def close(actual, expected, name):
    finite(actual, name)
    finite(expected, name)
    require(math.isclose(actual, expected, rel_tol=1e-10, abs_tol=1e-8),
            f"numeric mismatch: {name}")


def pair(value, name):
    require(type(value) is list and len(value) == 2, f"invalid pair: {name}")
    for item in value:
        finite(item, name)
    return value


def hex64(value, name):
    require(type(value) is str and re.fullmatch(r"[0-9a-f]{64}", value) is not None,
            f"invalid hash: {name}")


def _config(data):
    require(type(data) is bytes, "config must be bytes")
    return tomllib.loads(data.decode("utf-8"))


def _normalized(raw):
    """Only defaults actually added to these two pinned diagnostic configs."""
    value = deepcopy(raw)
    value["game"]["tree"].setdefault("params", {})
    algorithm = value["algorithm"]
    for key, default in {"alpha": 1.5, "beta": 0.0, "gamma": 3.0,
                         "pow4_reset": True}.items():
        algorithm.setdefault(key, default)
    return value


def _diagnostic_config(value):
    require(set(value) == {"schema", "game", "rake", "utility", "algorithm", "run"},
            "unexpected config sections")
    require(value["schema"] == "solvers.postflop/v1", "wrong config schema")
    game = value["game"]
    require(len(game["board"].split()) == 5, "not a River board")
    for key in ("pot", "effective_stack", "min_bet"):
        integer(game[key], key, 1)
    require(game["iso_merging"] is False, "iso merging changed")
    tree = game["tree"]
    require(tree["kind"] == "script" and type(tree["script"]) is str
            and tree["script"].strip(), "missing inline tree script")
    require("source" not in tree and "allin_threshold" not in tree,
            "external tree source or size threshold")
    require(tree["include_allin"] is False and tree.get("params", {}) == {},
            "tree defaults changed")
    require(value["utility"] == {"kind": "chip-ev"}, "utility changed")
    rake = value["rake"]
    require(set(rake) == {"kind", "rate", "cap", "no_flop_no_drop"}, "rake shape")
    require(rake["kind"] == "percent-cap" and rake["rate"] == 0.05
            and rake["no_flop_no_drop"] is False, "rake changed")
    require(finite(rake["cap"], "rake cap") > 0, "nonpositive rake cap")
    require(value["algorithm"].get("schedule") == "dcfr", "schedule changed")
    run = value["run"]
    require(run == {"iterations": 10000, "check_every": 100, "max_time": "30s",
                    "storage": "f32", "threads": 1, "par_chance_depth": 0},
            "finite diagnostic run contract changed")
    for key in ("iterations", "check_every", "threads", "par_chance_depth"):
        integer(run[key], key)


def check_validate(report, effective_toml_bytes, rawtomlbytes):
    raw, effective = _config(rawtomlbytes), _config(effective_toml_bytes)
    _diagnostic_config(raw)
    _diagnostic_config(effective)
    require(effective == _normalized(raw), "normalization changed config semantics")
    require(report.get("status") == "valid"
            and report.get("schema") == "solvers.postflop/v1"
            and report.get("gameKind") == "postflop", "native validation failed")
    require(report.get("effectiveConfig") == effective, "effective JSON/TOML mismatch")
    require(report.get("profile") ==
            "vector CFR average profile; general-sum utilities, no Nash convergence guarantee",
            "wrong general-sum profile")
    require(type(report.get("tree")) is dict, "missing native lowered-tree diagnostic")
    return {"status": "valid", "self_contained": True,
            "zero_sum_terminal_utility": False, "external_acceptance": None}


def check_tree(actual, expected):
    """Bind every exported decision row, then reconstruct River edge closure."""
    require(type(actual) is list and type(expected["rows"]) is list, "tree must be a list")
    fields = {"history", "street", "actor", "pot", "actions", "stored"}

    def indexed(rows):
        result = {}
        for row in rows:
            require(type(row) is dict and set(row) == fields, "tree row shape")
            history = row["history"]
            require(type(history) is str and history not in result, "duplicate/invalid history")
            require(row["street"] == "river" and row["actor"] in ("oop", "ip")
                    and row["stored"] is True, "tree row binding")
            integer(row["pot"], "pot", 1)
            require(type(row["actions"]) is list and row["actions"]
                    and all(type(a) is str for a in row["actions"]), "invalid action labels")
            require(len(set(row["actions"])) == len(row["actions"]), "duplicate actions")
            result[history] = row
        return result

    rows, wanted = indexed(actual), indexed(expected["rows"])
    require(rows == wanted, "exported tree differs from observed decision rows")
    require("" in rows, "missing root")
    terminals, seen = {}, set()
    pending = [("", (0, 0), 0)]
    edge_count = 0
    while pending:
        history, paid, actor = pending.pop()
        require(history in rows and history not in seen, "missing/repeated decision")
        seen.add(history)
        row = rows[history]
        require(row["actor"] == ("oop", "ip")[actor], "actor mismatch")
        require(row["pot"] == expected["pot_chips"] + sum(paid), "pot/raise-to mismatch")
        owed = paid[1 - actor] - paid[actor]
        require(owed >= 0, "negative amount to call")
        for action in row["actions"]:
            edge_count += 1
            next_paid = list(paid)
            kind = None
            if action == "fold":
                require(owed > 0, "fold without a wager")
                token, kind = "f", "fold"
            elif action == "call":
                require(owed > 0, "call without a wager")
                token, kind = "c", "call"
            elif action == "check":
                require(owed == 0, "check facing a wager")
                token = "x"
                if history.endswith("x"):
                    kind = "check_check"
            else:
                match = re.fullmatch(r"(bet |raise to )([1-9][0-9]*)", action)
                require(match is not None, "invalid native wager label")
                require(match[1] == ("bet " if owed == 0 else "raise to "),
                        "bet/raise label mismatch")
                target = int(match[2])
                require(paid[1 - actor] < target <= expected["stack_chips"],
                        "wager target outside remaining stack")
                next_paid[actor] = target
                token = "r" + str(target)
            child = history + token
            if kind is None:
                require(child not in terminals, "decision/terminal collision")
                pending.append((child, tuple(next_paid), 1 - actor))
            else:
                require(child not in rows and child not in terminals, "terminal collision")
                terminals[child] = kind
    require(seen == set(rows), "unreachable exported decisions")
    require(terminals == expected["terminal_histories"], "terminal graph differs")
    counts = {"decision_nodes": len(rows), "action_edges": edge_count,
              "terminal_nodes": len(terminals), "public_nodes": len(rows) + len(terminals)}
    require(counts["action_edges"] + 1 == counts["public_nodes"], "not a closed tree")
    for key, value in counts.items():
        require(integer(expected[key], key, 1) == value, f"wrong {key}")
    return {**counts, "terminal_kinds": dict(sorted(Counter(terminals.values()).items())),
            "terminal_evidence": "derived River edges; CLI exports decision rows only"}


def _live(live):
    require(live.get("kind") == "postflop", "wrong live game")
    iteration = integer(live["iterations"], "live iterations", 100)
    require(iteration <= 10000 and iteration % 100 == 0, "live iteration bound/cadence")
    require(finite(live["wallSecs"], "wallSecs") >= 0, "negative wall time")
    close(live["nashConv"], finite(live["explP0"], "explP0")
          + finite(live["explP1"], "explP1"), "live gain sum")
    return iteration


def check_solve(live, manifest, progress, effectivebytes, runconfigbytes):
    require(effectivebytes == runconfigbytes, "run config bytes differ from validated effective config")
    effective, runconfig = _config(effectivebytes), _config(runconfigbytes)
    _diagnostic_config(effective)
    _diagnostic_config(runconfig)
    require(_normalized(runconfig) == effective, "solve config differs from validated config")
    require(manifest.get("schemaVersion") == 1 and manifest.get("state") == "completed"
            and manifest.get("completion") == "completed"
            and manifest.get("gameKind") == "postflop"
            and manifest.get("configSchema") == "solvers.postflop/v1"
            and manifest.get("failure") is None, "solve not completed")
    hex64(manifest["configHash"], "manifest configHash")
    iteration = _live(live)
    require(type(progress) is list and len(progress) == iteration // 100,
            "missing/extra progress samples")
    previous = 0.0
    for index, row in enumerate(progress):
        require(integer(row["iteration"], "progress iteration") == (index + 1) * 100,
                "progress cadence")
        elapsed = finite(row["elapsed_secs"], "progress elapsed")
        require(elapsed >= previous, "progress time regressed")
        close(row["nash_conv"], finite(row["expl_p0"], "progress expl_p0")
              + finite(row["expl_p1"], "progress expl_p1"), "progress gain sum")
        if index != len(progress) - 1:
            require(elapsed < 30.0, "continued after sampled time limit")
        previous = elapsed
    last = progress[-1]
    for native, final in (("expl_p0", "explP0"), ("expl_p1", "explP1"),
                          ("nash_conv", "nashConv")):
        close(last[native], live[final], "last progress/live " + native)
    require(live["wallSecs"] + 1e-8 >= previous, "wall time precedes final sample")
    require(iteration == 10000 or previous >= 30.0, "early stop without sampled time limit")
    return {"iterations": iteration, "samples": len(progress), "last_elapsed_secs": previous,
            "budget_boundary": "iteration_cap" if iteration == 10000 else "sampled_time_limit",
            "target_configured": False, "external_acceptance": None}


def check_audit(report, live, expected, solpin):
    iteration = _live(live)
    require(report.get("schema") == "solvers.research.hu-saved-profile-audit/v1",
            "saved audit schema")
    require(report.get("threads") == 1 and report.get("par_chance_depth") == 0
            and report.get("par_min_children") == 12, "saved audit parallel settings")
    require(report.get("pot_chips") == expected["pot_chips"]
            and report.get("effective_stack_chips") == expected["stack_chips"], "audit game")
    require(report.get("rake") == {"kind": "percent-cap", "rate": 0.05,
                                   "cap": expected["cap_chips"], "no_flop_no_drop": False},
            "audit rake")
    require(report.get("utility") == {"kind": "chip-ev"}
            and report.get("zero_sum_terminal_utility") is False
            and report.get("value_basis") == "subgame_start_utility", "audit value basis")
    require(pair(report["ev_offset"], "ev_offset") ==
            [expected["pot_chips"] // 2, expected["pot_chips"] - expected["pot_chips"] // 2],
            "audit subgame offset")
    artifact = report["artifact"]
    require(artifact["path"] == solpin["path"]
            and integer(artifact["bytes"], "artifact bytes", 1) == solpin["bytes"], "artifact identity")
    hex64(artifact["blake3"], "native artifact blake3")
    hex64(artifact["config_blake3"], "native config blake3")
    require(artifact["config_blake3"] == solpin["config_blake3"], "SOL header config binding")
    require(artifact["format_version"] == 4 and artifact["mode"] == "full"
            and artifact["source_storage"] == "f32" and artifact["iterations"] == iteration,
            "artifact format/storage/iteration")
    require(artifact["stored_nodes"] == expected["decision_nodes"]
            and artifact["node_count"] == expected["public_nodes"], "artifact tree count")
    meta = report["pre_save_metadata"]
    require(meta["iterations"] == iteration and meta["storage"] == "f32", "pre-save metadata")
    pair(meta["ev"], "pre-save EV")
    for actual, final in zip(pair(meta["expl"], "pre-save gains"), (live["explP0"], live["explP1"])):
        close(actual, final, "pre-save/live gain")
    close(meta["nash_conv"], live["nashConv"], "pre-save/live NashConv")
    close(meta["wall_secs"], live["wallSecs"], "pre-save/live wall")
    recomputed = report["recomputed"]
    require(recomputed["profile"] == "stored_quantized", "wrong saved profile")
    ev, br, gains = (pair(recomputed[key], key) for key in ("ev", "br", "gains"))
    for index in range(2):
        close(gains[index], br[index] - ev[index], "stored BR minus EV")
    close(recomputed["nash_conv"], sum(gains), "stored gain sum")
    for key in ("input_hash_secs", "load_secs", "eval_secs"):
        require(finite(report[key], key) >= 0, "negative audit timing")
    return {"profile": "stored_quantized", "live_nash_conv": live["nashConv"],
            "stored_nash_conv": recomputed["nash_conv"],
            "zero_sum_terminal_utility": False, "external_acceptance": None,
            "artifact_blake3_binding": "native reported only; caller binds bytes with SHA-256",
            "config_binding": "native config digest equals independently read SOL header"}
