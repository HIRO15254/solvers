"""Prepare a reversible CFR root-action-only research copy; no Cargo or solve."""
from pathlib import Path
import argparse
import difflib
import hashlib
import json
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SOURCE = "crates/engine/src/solver.rs"
SOURCE_SHA = "69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a"
ANCHOR = "impl ActionPlan {\n"
STEP = "    pub fn step(&mut self) {\n        let action_plan = ActionPlan::new(&self.game.tree);"
RUN = "        // Reuse the immutable plan across both player passes and iterations.\n        let action_plan = ActionPlan::new(&self.game.tree);"

def pin(data):
    return {"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()}

def replace_once(source, old, new):
    if source.count(old) != 1:
        raise ValueError("unique source anchor differs")
    return source.replace(old,new,1)

def transform(original, method, tests):
    if pin(original)["sha256"] != SOURCE_SHA:
        raise ValueError("production source pin differs")
    text = original.decode("utf-8")
    text = replace_once(text,ANCHOR,ANCHOR+method)
    for anchor in (STEP,RUN):
        text=replace_once(text,anchor,anchor.replace("ActionPlan::new", "ActionPlan::for_cfr"))
    return (text+tests).encode("utf-8")

def inverse(candidate, method, tests):
    text=candidate.decode("utf-8")
    if not text.endswith(tests):
        raise ValueError("test appendix differs")
    text=text[:-len(tests)]
    for anchor in (RUN,STEP):
        text=replace_once(text,anchor.replace("ActionPlan::new", "ActionPlan::for_cfr"),anchor)
    text=replace_once(text,ANCHOR+method,ANCHOR)
    return text.encode("utf-8")

def verify_scope(original, candidate, method, tests):
    if inverse(candidate,method,tests) != original:
        raise ValueError("change outside added plan, two CFR call sites and tests")
    before,after=original.decode(),candidate.decode()
    for start,end in (("fn cfr_pass<","/// Read-only context"),("fn value_pass<","#[cfg(test)]")):
        if before[before.index(start):before.index(end,before.index(start))] != after[after.index(start):after.index(end,after.index(start))]:
            raise ValueError("traversal arithmetic or borrow structure changed")
    return {"inverse_exact":True,"cfr_and_value_traversals_unchanged":True,
            "quality_plan_factory_unchanged":True,"native_test_execution":False}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check",action="store_true")
    args=parser.parse_args()
    original=(ROOT/SOURCE).read_bytes()
    method=(HERE/"root_plan.rs.in").read_text(encoding="utf-8")
    tests=(HERE/"plan_tests.rs.in").read_text(encoding="utf-8")
    generated=transform(original,method,tests)
    # rustfmt parses/formats only; no compiler, linker or workload is invoked.
    version=subprocess.run(["rustfmt","--version"],capture_output=True,check=True,timeout=10)
    formatted=subprocess.run(["rustfmt","--edition","2024","--emit","stdout"],input=generated,capture_output=True,check=True,timeout=10)
    candidate=formatted.stdout
    checks=verify_scope(original,candidate,method,tests)
    difference="".join(difflib.unified_diff(original.decode().splitlines(True),candidate.decode().splitlines(True),"a/"+SOURCE,"b/"+SOURCE)).encode()
    record={"schema":"r1.root-action-only-preparation/v1","source":{"path":SOURCE,**pin(original)},
            "preparation_controls":{name:pin((HERE/name).read_bytes()) for name in ("prepare.py","root_plan.rs.in","plan_tests.rs.in")},
            "candidate":pin(candidate),"patch":pin(difference),"formatter":{"version":version.stdout.decode().strip(),"stderr":formatted.stderr.decode()},
            "scope":checks,"production_adopted":False,"compiled":None,"quality":None,"performance":None}
    outputs={"solver.rs":candidate,"candidate.patch":difference,"provenance.json":(json.dumps(record,indent=2)+"\n").encode()}
    if (ROOT/SOURCE).read_bytes()!=original:
        raise ValueError("source changed during preparation")
    for name,data in outputs.items():
        path=HERE/name
        if args.check:
            if path.read_bytes()!=data:raise ValueError("retained candidate differs: "+name)
        else:
            with path.open("xb") as stream:stream.write(data)
    print(json.dumps({"status":"passed","candidate":pin(candidate),"checks":checks}))

if __name__=="__main__":
    main()
