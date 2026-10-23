"""Fills `viz/page.html` with the cases and their layouts, into `viz/tidy3d.html`.

Reads `viz/out/{tenk,funnel,first,random}.json` (what each node is), the tree files they
were written with (its box), and `viz/out/layouts.json` (where `examples/space_trees.rs`
put it, in the same order).
"""
import json
import os

from pg_common import HERE, REPO

CASES = [
    ("tenk", os.path.join(REPO, "tests", "data", "pg_tenk.tree")),
    ("funnel", os.path.join(REPO, "tests", "data", "pg_funnel.tree")),
    ("first", os.path.join(REPO, "tests", "data", "pg_first.tree")),
    ("random", os.path.join(HERE, "out", "random.tree")),
]


def rows(path):
    return [list(map(float, line.split())) for line in open(path) if line.strip() and not line.startswith("#")]


layouts = json.load(open(os.path.join(HERE, "out", "layouts.json")))
assert len(layouts) == len(CASES)

data = []
for (key, tree), lay in zip(CASES, layouts):
    c = json.load(open(os.path.join(HERE, "out", f"{key}.json")))
    boxes = rows(tree)
    nodes = []
    for i, r in enumerate(boxes):
        n = [round(r[0], 4), r[1], round(r[2], 4), int(r[4])]
        if c["meta"] is not None:
            n += c["meta"][i]
        nodes.append(n)
    data.append(dict(key=key, title=c["title"], blurb=c["blurb"], time=c["time"], facts=c["facts"],
                     nodes=nodes, cross=c["cross"], y=lay["y"],
                     layouts={rule: lay[rule] for rule in ("alternate", "compact", "allx")}))

page = open(os.path.join(HERE, "page.html")).read()
assert page.count("__DATA__") == 1
out = page.replace("__DATA__", json.dumps(data, separators=(",", ":")))
open(os.path.join(HERE, "tidy3d.html"), "w").write(out)
print(f"viz/tidy3d.html: {len(out):,} bytes, {sum(len(c['nodes']) for c in data):,} nodes in {len(data)} cases")
