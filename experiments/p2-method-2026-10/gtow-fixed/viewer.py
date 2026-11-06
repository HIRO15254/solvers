"""Write a self-contained HTML viewer of p2-class-profile Preflop strategies, side by side with GTO Wizard.

usage: python viewer.py --config TOML [--reference JSON] --out HTML <label>=<p2-class-profile JSON> [...]

Every profile must come from the same tree. The page walks the tree by action, draws each node's 13x13 grid
(pairs on the diagonal, suited above it) as stacked action bars, and dims classes by the actor's reach under the
selected profile. Where the node is one of GTO Wizard's reference nodes (matched as in b4s-8h/compare.py), its
strategy, the distance and the EV loss are shown beside ours. Line probabilities multiply each seat's combo-
weighted reach and ignore card removal, so they are approximate. Python standard library only.
"""
import argparse
import html
import json
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "b4s-8h"))
from compare import REFERENCE, RANKS, compare, load, our_index  # noqa: E402

COMBOS = [6 if r == c else 4 if r < c else 12 for r in range(13) for c in range(13)]


def class_name(i):
    r, c = divmod(i, 13)
    if r == c:
        return RANKS[r] * 2
    return RANKS[min(r, c)] + RANKS[max(r, c)] + ("s" if r < c else "o")


def reaches(nodes, seats):
    """Per path: every seat's own reach per class, following the profile's average strategy."""
    out = {(): [[1.0] * 169 for _ in range(seats)]}
    for path in sorted(nodes, key=len):
        node = nodes[path]
        base = out[path]
        for a, action in enumerate(node["actions"]):
            child = path + (action,)
            if child not in nodes:
                continue
            reach = [list(r) for r in base]
            row = reach[node["actor"]]
            for c in range(169):
                row[c] *= node["probabilities"][c][a]
            out[child] = reach
    return out


def permille(values):
    return [round(1000 * v) for v in values]


def build(profiles, positions, reference):
    """The page data: one entry per tree node, with each profile's strategy and reach."""
    first = profiles[0][1]
    for label, nodes in profiles[1:]:
        if set(nodes) != set(first) or any(nodes[p]["actions"] != first[p]["actions"] for p in first):
            raise SystemExit(f"{label}: tree differs from {profiles[0][0]}")
    paths = sorted(first, key=lambda p: (len(p), p))
    index = {p: i for i, p in enumerate(paths)}
    reach = {label: reaches(nodes, len(positions)) for label, nodes in profiles}
    gtow = {}
    if reference is not None:
        order = [our_index(c) for c in reference["classes"]]
        for label, nodes in profiles:
            for r in compare(reference, nodes):
                if not r["matched"]:
                    continue
                path = tuple(r["path"])
                entry = gtow.setdefault(path, {
                    "history": " ".join(r["node"]["history"]) or "(root)",
                    "lp": r["node"]["line_probability"],
                    "range": [0] * 169,
                    "s": [[0] * len(first[path]["actions"]) for _ in range(169)],
                    "approx": r["approximate"],
                    "cmp": {},
                })
                columns = [first[path]["actions"].index(x) for x in r["labels"]]
                for k, ours in enumerate(order):
                    entry["range"][ours] = round(1000 * r["node"]["range"][k])
                    for j, col in enumerate(columns):
                        entry["s"][ours][col] = round(1000 * r["node"]["actions"][j]["strategy"][k])
                entry["cmp"][label] = {
                    "distance": round(r["distance"], 4),
                    "loss_bb100": round(100 * r["node"]["line_probability"] * r["loss"], 4),
                    "ours": {first[path]["actions"][c]: round(f, 4) for c, f in zip(columns, r["ours"])},
                    "ref": {first[path]["actions"][c]: round(f, 4) for c, f in zip(columns, r["reference"])},
                }
    out = []
    for path in paths:
        node = first[path]
        kids = {a: index[path + (a,)] for a in node["actions"] if path + (a,) in index}
        per = {}
        for label, nodes in profiles:
            seat_reach = reach[label][path]
            lp = 1.0
            for row in seat_reach:
                lp *= sum(w * x for w, x in zip(COMBOS, row)) / 1326
            per[label] = {
                "s": [permille(row) for row in nodes[path]["probabilities"]],
                "r": permille(seat_reach[node["actor"]]),
                "lp": float(f"{lp:.3g}"),
            }
        item = {"path": list(path), "actor": positions[node["actor"]], "acts": node["actions"], "kids": kids, "p": per}
        if path in gtow:
            item["g"] = gtow[path]
        out.append(item)
    return out


PAGE = """<!doctype html>
<html lang="ja"><head><meta charset="utf-8"><title>P2 Preflop viewer</title>
<style>
body{font:14px system-ui,sans-serif;margin:0;display:flex;height:100vh;color:#222}
#side{width:300px;overflow:auto;border-right:1px solid #ccc;padding:8px;box-sizing:border-box;font-size:12px}
#side div{cursor:pointer;padding:2px 4px;border-radius:3px}#side div:hover{background:#eef}
#side .cur{background:#dde}
#main{flex:1;overflow:auto;padding:12px}
.crumbs span{cursor:pointer;color:#036}.crumbs span:hover{text-decoration:underline}
.kids button{margin:2px;padding:4px 8px;border:1px solid #888;border-radius:4px;cursor:pointer;background:#fff}
.kids button:disabled{cursor:default;color:#999}
.grids{display:flex;gap:24px;flex-wrap:wrap;margin-top:10px}
table.g{border-collapse:collapse}table.g td{width:34px;height:26px;padding:0;border:1px solid #fff;position:relative}
td .bar{position:absolute;inset:0;display:flex}td .bar i{display:block;height:100%}
td b{position:absolute;left:2px;top:1px;font-size:10px;font-weight:600;color:#000;text-shadow:0 0 2px #fff}
.legend span{display:inline-block;margin-right:10px}.legend i{display:inline-block;width:12px;height:12px;margin-right:3px;vertical-align:middle}
table.f{border-collapse:collapse;margin-top:6px}table.f td,table.f th{border:1px solid #ccc;padding:2px 6px;text-align:right}
#tip{position:fixed;background:#fff;border:1px solid #888;padding:6px;font-size:12px;pointer-events:none;display:none;box-shadow:2px 2px 6px #0003}
.note{color:#666;font-size:12px}
</style></head><body>
<div id="side"></div><div id="main"></div><div id="tip"></div>
<script>
const D=__DATA__;
const LABELS=Object.keys(D.nodes[0].p);let cur=0,prof=LABELS[LABELS.length-1],dim=true;
const RANKS="AKQJT98765432";
function cname(i){const r=Math.floor(i/13),c=i%13;if(r==c)return RANKS[r]+RANKS[r];return RANKS[Math.min(r,c)]+RANKS[Math.max(r,c)]+(r<c?"s":"o")}
const COMBOS=[...Array(169).keys()].map(i=>{const r=Math.floor(i/13),c=i%13;return r==c?6:r<c?4:12});
function bb(x){return (x/1000).toFixed(x%1000?(x%100?3:1):0).replace(/\\.?0+$/,"")}
function aname(a){const p=a.split(":");if(p[0]=="fold")return"Fold";if(p[0]=="check")return"Check";if(p[0]=="call")return"Call";
 if(p[0]=="raise-to")return(p[2]=="all-in"?"All-in ":"Raise ")+bb(+p[1])+"bb";return a}
function color(a,acts){if(a=="fold")return"#6d8db8";if(a.startsWith("call")||a=="check")return"#5cb85c";
 if(a.endsWith(":all-in"))return"#7a0019";const r=acts.filter(x=>x.startsWith("raise-to")&&!x.endsWith(":all-in"));
 const k=r.indexOf(a),n=r.length;const L=[["#e8743b"],["#f0a35e","#d9534f"],["#f5c27a","#e8743b","#c9302c"]][Math.min(n,3)-1]||["#d9534f"];return L[Math.min(k,L.length-1)]}
function pathText(n){let s=[];let p=[];for(const a of n.path){const node=D.nodes[find(p)];s.push(node.actor+" "+aname(a));p=p.concat([a])}return s}
const IDX=new Map(D.nodes.map((n,i)=>[n.path.join("/"),i]));function find(p){return IDX.get(p.join("/"))}
function freqs(n,s,r){const out=n.acts.map(()=>0);let t=0;for(let c=0;c<169;c++){const w=COMBOS[c]*r[c];t+=w;n.acts.forEach((_,a)=>out[a]+=w*s[c][a]/1000)}return out.map(x=>t?x/t:0)}
function grid(n,s,r,title){let h=`<div><div><b>${title}</b></div><table class="g">`;
 for(let i=0;i<13;i++){h+="<tr>";for(let j=0;j<13;j++){const c=i*13+j;const op=dim?Math.max(0.08,r[c]/1000):1;
  h+=`<td data-c="${c}"><div class="bar" style="opacity:${op}">`+n.acts.map((a,k)=>`<i style="width:${s[c][k]/10}%;background:${color(a,n.acts)}"></i>`).join("")+`</div><b>${cname(c)}</b></td>`}h+="</tr>"}
 return h+"</table></div>"}
function render(){const n=D.nodes[cur],P=n.p[prof],g=n.g;
 let crumbs=[`<span data-go="0">root</span>`];let p=[];for(const a of n.path){const node=D.nodes[find(p)];p=p.concat([a]);crumbs.push(`<span data-go="${find(p)}">${node.actor} ${aname(a)}</span>`)}
 let h=`<div class="crumbs">${crumbs.join(" → ")}</div>`;
 h+=`<h2>${n.actor} の手番</h2><div class="note">この線の到達確率（概算、card removal を無視）: ${LABELS.map(l=>l+" "+n.p[l].lp).join(" / ")}${g?" ・ GTO Wizard "+g.lp.toFixed(4):""}</div>`;
 h+=`<div style="margin:6px 0">表示する解: `+LABELS.map(l=>`<label><input type="radio" name="prof" value="${l}" ${l==prof?"checked":""}> ${l}</label> `).join("")+
  ` ・ <label><input type="checkbox" id="dim" ${dim?"checked":""}> rangeで薄くする</label></div>`;
 h+=`<div class="kids">次の行動: `+n.acts.map(a=>`<button ${a in n.kids?`data-go="${n.kids[a]}"`:"disabled title='ここで終わる（Flopへ、または全員fold）'"} style="border-left:6px solid ${color(a,n.acts)}">${aname(a)}</button>`).join("")+"</div>";
 h+=`<div class="legend" style="margin-top:6px">`+n.acts.map(a=>`<span><i style="background:${color(a,n.acts)}"></i>${aname(a)}</span>`).join("")+"</div>";
 const fo=freqs(n,P.s,P.r);
 h+=`<table class="f"><tr><th></th>`+n.acts.map(a=>`<th>${aname(a)}</th>`).join("")+"</tr>";
 h+=`<tr><td>${prof}（自分のrange）</td>`+fo.map(x=>`<td>${(100*x).toFixed(1)}%</td>`).join("")+"</tr>";
 if(g){const c=g.cmp[prof];if(c){h+=`<tr><td>${prof}（GTO Wizardのrange）</td>`+n.acts.map(a=>`<td>${a in c.ours?(100*c.ours[a]).toFixed(1)+"%":"—"}</td>`).join("")+"</tr>";
  h+=`<tr><td>GTO Wizard</td>`+n.acts.map(a=>`<td>${a in c.ref?(100*c.ref[a]).toFixed(1)+"%":"—"}</td>`).join("")+"</tr>"}}
 h+="</table>";
 if(g){h+=`<div class="note">GTO Wizardとの比較（GTO Wizardのrangeで重み付け）: `+LABELS.filter(l=>g.cmp[l]).map(l=>`${l} 距離 ${g.cmp[l].distance.toFixed(3)}・EV損失 ${g.cmp[l].loss_bb100.toFixed(3)} bb/100`).join(" ／ ")+(g.approx?" ・ sizeを近い値に対応付けたnode":"")+"</div>"}
 else h+=`<div class="note">このnodeはGTO Wizardの参照（135 node）にない。</div>`;
 h+=`<div class="grids">`+grid(n,P.s,P.r,prof);
 if(g)h+=grid(n,g.s,g.range,"GTO Wizard");
 h+="</div>";
 document.getElementById("main").innerHTML=h;side()}
function side(){const tot=LABELS.map(l=>l+" "+D.nodes.reduce((t,n)=>t+(n.g&&n.g.cmp[l]?n.g.cmp[l].loss_bb100:0),0).toFixed(3)).join(" / ");
 let h=`<div class="note">GTO Wizardとの差の合計（EV損失 bb/100）: ${tot}</div><b>GTO Wizardの主なnode（到達確率順）</b>`;
 D.nodes.map((n,i)=>[n,i]).filter(([n])=>n.g).sort((a,b)=>b[0].g.lp-a[0].g.lp).forEach(([n,i])=>{
  const c=n.g.cmp[prof];h+=`<div data-go="${i}" class="${i==cur?"cur":""}">${n.actor}: ${pathText(n).join(", ")||"open"}<br><span class="note">p=${n.g.lp.toFixed(4)}${c?" ・ 損失 "+c.loss_bb100.toFixed(3):""}</span></div>`});
 document.getElementById("side").innerHTML=h}
document.addEventListener("click",e=>{const t=e.target.closest("[data-go]");if(t){cur=+t.dataset.go;render()}});
document.addEventListener("change",e=>{if(e.target.name=="prof"){prof=e.target.value;render()}if(e.target.id=="dim"){dim=e.target.checked;render()}});
const tip=document.getElementById("tip");
document.addEventListener("mousemove",e=>{const td=e.target.closest("td[data-c]");if(!td){tip.style.display="none";return}
 const c=+td.dataset.c,n=D.nodes[cur];let h=`<b>${cname(c)}</b><table class="f"><tr><th></th>`+LABELS.map(l=>`<th>${l}</th>`).join("")+(n.g?"<th>GTO Wizard</th>":"")+"</tr>";
 h+=`<tr><td>range</td>`+LABELS.map(l=>`<td>${(n.p[l].r[c]/10).toFixed(1)}%</td>`).join("")+(n.g?`<td>${(n.g.range[c]/10).toFixed(1)}%</td>`:"")+"</tr>";
 n.acts.forEach((a,k)=>{h+=`<tr><td>${aname(a)}</td>`+LABELS.map(l=>`<td>${(n.p[l].s[c][k]/10).toFixed(1)}%</td>`).join("")+(n.g?`<td>${(n.g.s[c][k]/10).toFixed(1)}%</td>`:"")+"</tr>"});
 tip.innerHTML=h+"</table>";tip.style.display="block";tip.style.left=Math.min(e.clientX+14,innerWidth-tip.offsetWidth-4)+"px";tip.style.top=Math.min(e.clientY+14,innerHeight-tip.offsetHeight-4)+"px"});
render();
</script></body></html>
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--config", required=True, type=Path, help="the tree's solvers.nlh/v1 TOML (seat order)")
    ap.add_argument("--reference", type=Path, default=REFERENCE, help="GTO Wizard reference JSON")
    ap.add_argument("--no-reference", action="store_true")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("profiles", nargs="+", metavar="LABEL=PROFILE")
    args = ap.parse_args()
    positions = list(tomllib.loads(args.config.read_text(encoding="utf-8"))["table"]["stacks"])
    profiles = []
    for spec in args.profiles:
        label, _, path = spec.partition("=")
        if not path:
            raise SystemExit(f"expected LABEL=PROFILE, got {spec!r}")
        profiles.append((html.escape(label), load(path)))
    reference = None if args.no_reference else json.loads(args.reference.read_text(encoding="utf-8"))
    data = {"nodes": build(profiles, positions, reference)}
    text = json.dumps(data, separators=(",", ":")).replace("</", "<\\/")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(PAGE.replace("__DATA__", text), encoding="utf-8", newline="\n")
    print(f"{args.out}: {len(data['nodes'])} nodes, {sum('g' in n for n in data['nodes'])} GTO Wizard nodes, "
          f"{args.out.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
